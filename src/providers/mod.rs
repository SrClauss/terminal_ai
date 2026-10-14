use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ProviderError {
    #[error("Erro na requisição HTTP: {0}")]
    Http(#[from] reqwest::Error),
    #[error("Erro da API: status {0}, mensagem: {1}")]
    Api(reqwest::StatusCode, String),
    #[error("Resposta vazia da IA")]
    EmptyResponse,
    #[error("Erro ao processar JSON: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandSuggestion {
    pub command: String,
    pub explanation: String,
    pub affects_parent_shell: bool,
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn generate_command(
        &self,
        prompt: &str,
        os_info: &str,
        language: &str,
        session_context: &str,
        custom_rules: Option<&str>,
    ) -> Result<CommandSuggestion, ProviderError>;

    /// Variante em streaming: emite deltas via `on_delta` e retorna a sugestão final.
    /// Default: sem streaming, delega para `generate_command`.
    #[allow(clippy::too_many_arguments)]
    async fn generate_command_stream(
        &self,
        prompt: &str,
        os_info: &str,
        language: &str,
        session_context: &str,
        custom_rules: Option<&str>,
        _on_delta: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<CommandSuggestion, ProviderError> {
        self.generate_command(prompt, os_info, language, session_context, custom_rules)
            .await
    }
}

pub mod openai_compatible;
pub mod sse;
pub mod stream_accumulator;
pub use openai_compatible::OpenAiCompatibleProvider;
pub use stream_accumulator::{AnthropicStreamAccumulator, OpenAiStreamAccumulator, StreamEvent};

/// Formata a duração da requisição para exibição amigável (ex: "850ms", "1.42s")
pub fn format_latency(duration: std::time::Duration) -> String {
    let millis = duration.as_millis();
    if millis < 1000 {
        format!("{millis}ms")
    } else {
        format!("{:.2}s", duration.as_secs_f64())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_format_latency_under_one_second() {
        assert_eq!(format_latency(Duration::from_millis(250)), "250ms");
        assert_eq!(format_latency(Duration::from_millis(999)), "999ms");
        assert_eq!(format_latency(Duration::from_millis(0)), "0ms");
    }

    #[test]
    fn test_format_latency_over_one_second() {
        assert_eq!(format_latency(Duration::from_millis(1000)), "1.00s");
        assert_eq!(format_latency(Duration::from_millis(1540)), "1.54s");
        assert_eq!(format_latency(Duration::from_millis(3210)), "3.21s");
    }
}
