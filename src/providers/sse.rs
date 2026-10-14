//! Parser SSE (Server-Sent Events) puro, sem I/O.
//!
//! Recebe chunks arbitrários de bytes (que podem cortar linhas, eventos ou até
//! caracteres UTF-8 no meio) e devolve os payloads `data:` completos.

/// Evento extraído do stream SSE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SseEvent {
    /// Payload de um evento (linhas `data:` concatenadas com `\n`).
    Data(String),
    /// Marcador de término `data: [DONE]` (padrão OpenAI).
    Done,
}

#[derive(Debug, Default)]
pub struct SseParser {
    /// Bytes ainda não terminados em `\n` (podem conter UTF-8 incompleto).
    buffer: Vec<u8>,
    /// Linhas `data:` acumuladas do evento corrente.
    data_lines: Vec<String>,
}

impl SseParser {
    pub fn new() -> Self {
        Self::default()
    }

    /// Alimenta o parser com um chunk e retorna os eventos completos encontrados.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();

        while let Some(pos) = self.buffer.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buffer.drain(..=pos).collect();
            line.pop(); // remove '\n'
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            let line = String::from_utf8_lossy(&line).into_owned();
            self.process_line(&line, &mut events);
        }
        events
    }

    /// Finaliza o stream, despachando um evento pendente sem linha em branco final.
    pub fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let rest = std::mem::take(&mut self.buffer);
            let line = String::from_utf8_lossy(&rest).trim_end_matches('\r').to_string();
            self.process_line(&line, &mut events);
        }
        self.dispatch(&mut events);
        events
    }

    fn process_line(&mut self, line: &str, events: &mut Vec<SseEvent>) {
        if line.is_empty() {
            self.dispatch(events);
            return;
        }
        if line.starts_with(':') {
            return; // comentário / keep-alive
        }
        let (field, value) = match line.find(':') {
            Some(idx) => {
                let v = &line[idx + 1..];
                (&line[..idx], v.strip_prefix(' ').unwrap_or(v))
            }
            None => (line, ""),
        };
        if field == "data" {
            self.data_lines.push(value.to_string());
        }
        // `event:`, `id:`, `retry:` são ignorados: os provedores repetem o tipo no JSON.
    }

    fn dispatch(&mut self, events: &mut Vec<SseEvent>) {
        if self.data_lines.is_empty() {
            return;
        }
        let data = self.data_lines.join("\n");
        self.data_lines.clear();
        if data.trim() == "[DONE]" {
            events.push(SseEvent::Done);
        } else {
            events.push(SseEvent::Data(data));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_all(chunks: &[&[u8]]) -> Vec<SseEvent> {
        let mut p = SseParser::new();
        let mut out = Vec::new();
        for c in chunks {
            out.extend(p.feed(c));
        }
        out.extend(p.finish());
        out
    }

    #[test]
    fn test_single_event_and_done() {
        let ev = feed_all(&[b"data: {\"a\":1}\n\ndata: [DONE]\n\n"]);
        assert_eq!(ev, vec![SseEvent::Data("{\"a\":1}".into()), SseEvent::Done]);
    }

    #[test]
    fn test_event_split_across_chunks() {
        let ev = feed_all(&[b"da", b"ta: {\"x\":", b"\"y\"}\n", b"\n"]);
        assert_eq!(ev, vec![SseEvent::Data("{\"x\":\"y\"}".into())]);
    }

    #[test]
    fn test_crlf_comments_and_event_field() {
        let ev = feed_all(&[b": keep-alive\r\nevent: content_block_delta\r\ndata: {\"t\":1}\r\n\r\n"]);
        assert_eq!(ev, vec![SseEvent::Data("{\"t\":1}".into())]);
    }

    #[test]
    fn test_utf8_split_mid_char() {
        let full = "data: explicação\n\n".as_bytes();
        let idx = full.iter().position(|&b| b == 0xC3).unwrap() + 1; // corta no meio do 'ç'
        let ev = feed_all(&[&full[..idx], &full[idx..]]);
        assert_eq!(ev, vec![SseEvent::Data("explicação".into())]);
    }

    #[test]
    fn test_multiline_data_and_pending_on_finish() {
        let ev = feed_all(&[b"data: a\ndata: b\n\ndata: tail"]);
        assert_eq!(ev, vec![SseEvent::Data("a\nb".into()), SseEvent::Data("tail".into())]);
    }

    #[test]
    fn test_byte_by_byte() {
        let src = b"data: 1\n\ndata: 2\n\ndata: [DONE]\n\n";
        let chunks: Vec<&[u8]> = src.chunks(1).collect();
        let ev = feed_all(&chunks);
        assert_eq!(
            ev,
            vec![SseEvent::Data("1".into()), SseEvent::Data("2".into()), SseEvent::Done]
        );
    }
}
