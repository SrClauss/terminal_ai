//! Acumulador de deltas SSE para protocolos OpenAI e Anthropic.
//!
//! Recebe payloads `data:` brutos extraídos pelo `SseParser`, decodifica os fragmentos
//! específicos de cada protocolo (tool calls em JSON ou texto com marcadores `%%%`),
//! emite deltas parciais para feedback em tempo real e consolida a `CommandSuggestion` final.

use crate::providers::openai_compatible::OpenAiCompatibleProvider;
use crate::providers::CommandSuggestion;
use serde::Deserialize;

/// Eventos emitidos incrementalmente durante a recepção do stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StreamEvent {
    /// Fragmento do comando gerado em tempo real.
    CommandDelta(String),
    /// Fragmento da explicação gerada em tempo real.
    ExplanationDelta(String),
}

/// Acumulador para streams compatíveis com OpenAI (ChatMessage chunk / delta).
/// Lida tanto com streaming de tool calls (`delta.tool_calls`) quanto com streaming de texto (`delta.content`).
#[derive(Default, Debug)]
pub struct OpenAiStreamAccumulator {
    /// Argumentos acumulados da chamada de função (ex: JSON com `command`, `explanation`, `affects_parent_shell`).
    raw_tool_arguments: String,
    /// Texto acumulado caso o modelo responda diretamente via `delta.content`.
    raw_content: String,
    /// Caracteres do comando já emitidos via evento de delta (para evitar duplicações).
    emitted_command_len: usize,
}

#[derive(Deserialize, Debug)]
struct OpenAiChunk {
    #[serde(default)]
    choices: Vec<OpenAiChunkChoice>,
}

#[derive(Deserialize, Debug)]
struct OpenAiChunkChoice {
    #[serde(default)]
    delta: OpenAiChunkDelta,
}

#[derive(Deserialize, Debug, Default)]
struct OpenAiChunkDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiChunkToolCall>>,
}

#[derive(Deserialize, Debug)]
struct OpenAiChunkToolCall {
    #[serde(default)]
    function: Option<OpenAiChunkFunction>,
}

#[derive(Deserialize, Debug, Default)]
struct OpenAiChunkFunction {
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize, Debug)]
struct ToolArgumentsPayload {
    command: Option<String>,
    explanation: Option<String>,
    affects_parent_shell: Option<bool>,
}

impl OpenAiStreamAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Alimenta o acumulador com uma linha de dados JSON (conteúdo de um evento SSE).
    pub fn feed_data(&mut self, json_str: &str) -> Vec<StreamEvent> {
        let chunk: OpenAiChunk = match serde_json::from_str(json_str) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        let mut events = Vec::new();

        for choice in chunk.choices {
            // Caso 1: Tool Calls (Function Calling streaming)
            if let Some(tool_calls) = choice.delta.tool_calls {
                for tc in tool_calls {
                    if let Some(func) = tc.function {
                        if let Some(args) = func.arguments {
                            self.raw_tool_arguments.push_str(&args);
                            if let Some(delta) = self.extract_new_command_delta() {
                                events.push(StreamEvent::CommandDelta(delta));
                            }
                        }
                    }
                }
            }

            // Caso 2: Resposta direta de conteúdo (ex: modelos sem suporte a tool calling)
            if let Some(content) = choice.delta.content {
                self.raw_content.push_str(&content);
                events.push(StreamEvent::CommandDelta(content));
            }
        }

        events
    }

    /// Tenta extrair incrementalmente os novos caracteres do campo `"command": "..."` nos argumentos JSON.
    fn extract_new_command_delta(&mut self) -> Option<String> {
        let partial_cmd = Self::extract_partial_json_field(&self.raw_tool_arguments, "command")?;
        if partial_cmd.len() > self.emitted_command_len {
            let new_part = partial_cmd[self.emitted_command_len..].to_string();
            self.emitted_command_len = partial_cmd.len();
            if !new_part.is_empty() {
                return Some(new_part);
            }
        }
        None
    }

    /// Extrator tolerante de valor string para JSON parcial/incompleto:
    /// Localiza `"key": "` e consome os caracteres com suporte a escapes básicos (`\"`, `\\`, `\n`).
    pub fn extract_partial_json_field(src: &str, field_name: &str) -> Option<String> {
        let key_pattern = format!("\"{}\"", field_name);
        let key_pos = src.find(&key_pattern)?;
        let rest = &src[key_pos + key_pattern.len()..];

        // Encontra o separador ':'
        let colon_pos = rest.find(':')?;
        let after_colon = rest[colon_pos + 1..].trim_start();

        // Deve iniciar com '"'
        if !after_colon.starts_with('"') {
            return None;
        }

        let content = &after_colon[1..];
        let mut result = String::new();
        let mut chars = content.chars().peekable();

        while let Some(ch) = chars.next() {
            if ch == '\\' {
                match chars.next() {
                    Some('"') => result.push('"'),
                    Some('\\') => result.push('\\'),
                    Some('n') => result.push('\n'),
                    Some('t') => result.push('\t'),
                    Some('r') => result.push('\r'),
                    Some(c) => {
                        result.push('\\');
                        result.push(c);
                    }
                    None => break, // Barra invertida no final do buffer
                }
            } else if ch == '"' {
                // Fim da string
                break;
            } else {
                result.push(ch);
            }
        }

        Some(result)
    }

    /// Consolida a sugestão final ao encerrar o stream SSE.
    pub fn finish(self) -> Option<CommandSuggestion> {
        // 1. Tentar parsear argumentos JSON estruturados
        if !self.raw_tool_arguments.trim().is_empty() {
            if let Ok(parsed) = serde_json::from_str::<ToolArgumentsPayload>(&self.raw_tool_arguments) {
                if let Some(cmd) = parsed.command {
                    let cmd = cmd.trim().to_string();
                    let expl = parsed.explanation.unwrap_or_default().trim().to_string();
                    let affects_parent_shell = parsed
                        .affects_parent_shell
                        .unwrap_or_else(|| OpenAiCompatibleProvider::detect_affects_parent_shell(&cmd));
                    return Some(CommandSuggestion {
                        command: cmd,
                        explanation: expl,
                        affects_parent_shell,
                    });
                }
            }

            // Fallback usando extrator tolerante caso o JSON tenha chegado ligeiramente truncado
            if let Some(cmd) = Self::extract_partial_json_field(&self.raw_tool_arguments, "command") {
                let cmd = cmd.trim().to_string();
                let expl = Self::extract_partial_json_field(&self.raw_tool_arguments, "explanation")
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                let affects = OpenAiCompatibleProvider::detect_affects_parent_shell(&cmd);
                return Some(CommandSuggestion {
                    command: cmd,
                    explanation: expl,
                    affects_parent_shell: affects,
                });
            }
        }

        // 2. Tentar extrair do texto plano/markdown acumulado
        if !self.raw_content.trim().is_empty() {
            return Some(OpenAiCompatibleProvider::extract_suggestion(&self.raw_content));
        }

        None
    }
}

/// Acumulador para streams Anthropic Messages API (`content_block_delta`, `message_delta`, etc.).
#[derive(Default, Debug)]
pub struct AnthropicStreamAccumulator {
    raw_content: String,
}

#[derive(Deserialize, Debug)]
struct AnthropicChunk {
    #[allow(dead_code)]
    #[serde(rename = "type")]
    event_type: Option<String>,
    #[serde(default)]
    delta: Option<AnthropicDelta>,
    #[serde(default)]
    error: Option<AnthropicErrorPayload>,
}

#[derive(Deserialize, Debug)]
struct AnthropicDelta {
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize, Debug)]
struct AnthropicErrorPayload {
    message: String,
}

impl AnthropicStreamAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    /// Alimenta o acumulador com evento JSON da Anthropic.
    pub fn feed_data(&mut self, json_str: &str) -> (Vec<StreamEvent>, Option<String>) {
        let chunk: AnthropicChunk = match serde_json::from_str(json_str) {
            Ok(c) => c,
            Err(_) => return (Vec::new(), None),
        };

        if let Some(err) = chunk.error {
            return (Vec::new(), Some(err.message));
        }

        let mut events = Vec::new();
        if let Some(delta) = chunk.delta {
            if let Some(text) = delta.text {
                self.raw_content.push_str(&text);
                events.push(StreamEvent::CommandDelta(text));
            }
        }

        (events, None)
    }

    /// Consolida a sugestão extraindo marcadores `%%%`.
    pub fn finish(self) -> Option<CommandSuggestion> {
        if self.raw_content.trim().is_empty() {
            None
        } else {
            Some(OpenAiCompatibleProvider::extract_suggestion(&self.raw_content))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_partial_json_field_incremental() {
        let s1 = r#"{"command": "git stat"#;
        assert_eq!(
            OpenAiStreamAccumulator::extract_partial_json_field(s1, "command"),
            Some("git stat".to_string())
        );

        let s2 = r#"{"command": "git status", "explanation": "mostra status"#;
        assert_eq!(
            OpenAiStreamAccumulator::extract_partial_json_field(s2, "command"),
            Some("git status".to_string())
        );
        assert_eq!(
            OpenAiStreamAccumulator::extract_partial_json_field(s2, "explanation"),
            Some("mostra status".to_string())
        );
    }

    #[test]
    fn test_openai_tool_calls_stream_accumulation() {
        let mut acc = OpenAiStreamAccumulator::new();

        let chunk1 = r#"{"choices":[{"delta":{"tool_calls":[{"function":{"arguments":"{\"command\": \"ls "}}]}}]}"#;
        let events1 = acc.feed_data(chunk1);
        assert_eq!(events1, vec![StreamEvent::CommandDelta("ls ".to_string())]);

        let chunk2 = r#"{"choices":[{"delta":{"tool_calls":[{"function":{"arguments":"-la\", \"explanation\": \"lista tudo\"}"}}]}}]}"#;
        let events2 = acc.feed_data(chunk2);
        assert_eq!(events2, vec![StreamEvent::CommandDelta("-la".to_string())]);

        let suggestion = acc.finish().expect("deve gerar sugestão final");
        assert_eq!(suggestion.command, "ls -la");
        assert_eq!(suggestion.explanation, "lista tudo");
        assert!(!suggestion.affects_parent_shell);
    }

    #[test]
    fn test_openai_content_fallback_stream() {
        let mut acc = OpenAiStreamAccumulator::new();

        let chunk1 = r#"{"choices":[{"delta":{"content":"%%%cd /tmp%%% "}}]}"#;
        let events1 = acc.feed_data(chunk1);
        assert_eq!(events1, vec![StreamEvent::CommandDelta("%%%cd /tmp%%% ".to_string())]);

        let chunk2 = r#"{"choices":[{"delta":{"content":"EXPLANATION: vai para tmp"}}]}"#;
        let events2 = acc.feed_data(chunk2);
        assert_eq!(events2, vec![StreamEvent::CommandDelta("EXPLANATION: vai para tmp".to_string())]);

        let suggestion = acc.finish().expect("sugestão");
        assert_eq!(suggestion.command, "cd /tmp");
        assert_eq!(suggestion.explanation, "vai para tmp");
        assert!(suggestion.affects_parent_shell);
    }

    #[test]
    fn test_anthropic_stream_accumulation() {
        let mut acc = AnthropicStreamAccumulator::new();

        let chunk1 = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"%%%cat file.txt"梁}"#;
        // json com erro sintatico é ignorado silenciosamente
        let (ev_err, _) = acc.feed_data(chunk1);
        assert!(ev_err.is_empty());

        let chunk2 = r#"{"type":"content_block_delta","delta":{"type":"text_delta","text":"%%%cat file.txt%%%\nEXPLANATION: lê arquivo"}}"#;
        let (events2, err) = acc.feed_data(chunk2);
        assert!(err.is_none());
        assert_eq!(events2.len(), 1);

        let suggestion = acc.finish().expect("sugestão anthropic");
        assert_eq!(suggestion.command, "cat file.txt");
        assert_eq!(suggestion.explanation, "lê arquivo");
    }
}
