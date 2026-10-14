use super::sse::{SseEvent, SseParser};
use super::stream_accumulator::{AnthropicStreamAccumulator, OpenAiStreamAccumulator, StreamEvent};
use super::{CommandSuggestion, LlmProvider, ProviderError};
use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

pub struct OpenAiCompatibleProvider {
    client: reqwest::Client,
    api_url: String,
    api_key: String,
    model: String,
}

impl OpenAiCompatibleProvider {
    pub fn new(api_url: String, api_key: String, model: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_url,
            api_key,
            model,
        }
    }

    pub fn build_endpoint_url(base_url: &str) -> String {
        let trimmed = base_url.trim_end_matches('/');
        if trimmed.ends_with("/chat/completions") || trimmed.ends_with("/messages") {
            trimmed.to_string()
        } else if trimmed.contains("anthropic.com") {
            if trimmed.ends_with("/v1") {
                format!("{trimmed}/messages")
            } else {
                format!("{trimmed}/v1/messages")
            }
        } else if trimmed.ends_with("/v1")
            || trimmed.ends_with("/openai")
            || trimmed.ends_with("/v4")
            || trimmed.ends_with("/v1beta")
        {
            format!("{trimmed}/chat/completions")
        } else {
            format!("{trimmed}/v1/chat/completions")
        }
    }

    pub fn build_headers(api_url: &str, api_key: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));

        if api_url.contains("anthropic.com") {
            headers.insert(
                HeaderName::from_static("x-api-key"),
                HeaderValue::from_str(api_key).unwrap_or(HeaderValue::from_static("")),
            );
            headers.insert(
                HeaderName::from_static("anthropic-version"),
                HeaderValue::from_static("2023-06-01"),
            );
        } else {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}"))
                    .unwrap_or(HeaderValue::from_static("")),
            );
        }
        headers
    }

    pub fn extract_suggestion(content: &str) -> CommandSuggestion {
        // Extração estrita e limpa usando exclusivamente o marcador %%%
        if let Some(start) = content.find("%%%") {
            let after_start = &content[start + 3..];
            if let Some(end) = after_start.find("%%%") {
                let command = after_start[..end].trim().trim_matches('`').trim().to_string();
                let raw_expl = after_start[end + 3..].trim();
                let explanation = if let Some(idx) = raw_expl.find("EXPLANATION:") {
                    raw_expl[idx + 12..].trim().to_string()
                } else if let Some(idx) = raw_expl.find("EXPLICAÇÃO:") {
                    raw_expl[idx + 12..].trim().to_string()
                } else {
                    raw_expl.to_string()
                };

                let affects_parent_shell = Self::detect_affects_parent_shell(&command);
                return CommandSuggestion {
                    command,
                    explanation,
                    affects_parent_shell,
                };
            }
        }

        // Fallback simples caso venha sem %%% (ex: bloco ```bash ... ```)
        if let Some(start) = content.find("```") {
            let after_ticks = &content[start + 3..];
            let code_start = if let Some(newline_pos) = after_ticks.find('\n') {
                newline_pos + 1
            } else {
                0
            };
            if let Some(end) = after_ticks[code_start..].find("```") {
                let block = after_ticks[code_start..code_start + end].trim();
                if let Some(first_line) = block.lines().find(|l| !l.trim().is_empty()) {
                    let cmd = first_line.trim().trim_start_matches("$ ").trim().to_string();
                    let expl = after_ticks[code_start + end + 3..].trim().to_string();
                    let affects_parent_shell = Self::detect_affects_parent_shell(&cmd);
                    return CommandSuggestion {
                        command: cmd,
                        explanation: expl,
                        affects_parent_shell,
                    };
                }
            }
        }

        let final_cmd = content.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string();
        let affects_parent_shell = Self::detect_affects_parent_shell(&final_cmd);
        CommandSuggestion {
            command: final_cmd,
            explanation: String::new(),
            affects_parent_shell,
        }
    }

    pub fn detect_affects_parent_shell(cmd: &str) -> bool {
        let trimmed = cmd.trim();
        trimmed.starts_with("cd ")
            || trimmed == "cd"
            || trimmed.starts_with("export ")
            || trimmed.starts_with("unset ")
            || trimmed.starts_with("source ")
            || trimmed.starts_with(". ")
            || trimmed.starts_with("alias ")
            || trimmed.starts_with("unalias ")
            || trimmed.contains("activate")
    }
}

#[derive(Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct FunctionParameters {
    #[serde(rename = "type")]
    param_type: String,
    properties: serde_json::Value,
    required: Vec<String>,
}

#[derive(Serialize)]
struct FunctionDefinition {
    name: String,
    description: String,
    parameters: FunctionParameters,
}

#[derive(Serialize)]
struct ToolDefinition {
    #[serde(rename = "type")]
    tool_type: String,
    function: FunctionDefinition,
}

#[derive(Serialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<Vec<ToolDefinition>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<serde_json::Value>,
    temperature: f32,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    stream: bool,
}

/// Callback para deltas de streaming.
pub type DeltaCallback<'a> = &'a mut (dyn FnMut(StreamEvent) + Send);

/// Consome um corpo SSE, repassando cada payload `data:` ao handler.
async fn read_sse(
    response: reqwest::Response,
    mut on_data: impl FnMut(&str) -> Result<(), ProviderError>,
) -> Result<(), ProviderError> {
    let mut parser = SseParser::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        for ev in parser.feed(&chunk) {
            match ev {
                SseEvent::Data(d) => on_data(&d)?,
                SseEvent::Done => return Ok(()),
            }
        }
    }
    for ev in parser.finish() {
        if let SseEvent::Data(d) = ev {
            on_data(&d)?;
        }
    }
    Ok(())
}

#[derive(Deserialize)]
struct FunctionCallResponse {
    name: String,
    arguments: String,
}

#[derive(Deserialize)]
#[allow(dead_code)]
struct ToolCallItem {
    id: Option<String>,
    #[serde(rename = "type")]
    call_type: Option<String>,
    function: FunctionCallResponse,
}

#[derive(Deserialize)]
struct ChatMessageResponse {
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallItem>>,
}

#[derive(Deserialize)]
struct ChatChoice {
    message: ChatMessageResponse,
}

#[derive(Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Deserialize, Debug)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicContent>,
}

#[derive(Deserialize, Debug)]
struct AnthropicContent {
    #[serde(rename = "type")]
    content_type: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct SuggestedCommandArguments {
    command: String,
    explanation: String,
    affects_parent_shell: Option<bool>,
}

impl OpenAiCompatibleProvider {
    async fn generate_inner(
        &self,
        prompt: &str,
        os_info: &str,
        language: &str,
        session_context: &str,
        custom_rules: Option<&str>,
        on_delta: Option<DeltaCallback<'_>>,
    ) -> Result<CommandSuggestion, ProviderError> {
        let rules_section = match custom_rules {
            Some(r) if !r.trim().is_empty() => format!("\n\nUSER CUSTOM PREFERENCES & RULES:\n{}", r.trim()),
            _ => String::new(),
        };

        let system_prompt = format!(
            r#"You are an expert Linux CLI assistant that converts natural language into precise, robust Bash commands.
Environment info: {os_info}.
Requested language for explanation: {language}.{session_context}{rules_section}

OUTPUT FORMAT INSTRUCTIONS (MANDATORY):
You MUST output the command enclosed strictly inside triple percent signs:
%%%<single bash command line>%%%
EXPLANATION: <brief bullet-point explanation in language '{language}'>

LINUX PATH & SHELL HEURISTICS (CRITICAL):
1. User directories: In Linux, personal directories are located in `/home/<username>/` (e.g. `~/Downloads`, `~/Screenshots`, `~/Pictures`).
   - If the user refers to "pasta pessoal", "home" or user folders while running as root, search in both `$HOME` and `/home/*/<folder>` (e.g., `find /home/* -type d -iname "Screenshots"`).
   - Never generate non-existent paths like `/home/downloads` or `/home/screenshots`. Use `find /home -type d -iname "screenshots"` or `~/Downloads`.
   - WSL (Windows Subsystem for Linux) Interoperability:
     * When running under WSL, Windows drives are mounted at `/mnt/<drive-letter>/` (e.g., `/mnt/c/`).
     * If the user refers to Windows files, Desktop, Downloads, or Documents, or provides Windows paths (e.g. `C:\Users\...`), ALWAYS resolve and convert them to `/mnt/c/Users/<WindowsUser>/...`.
     * Windows executables can be called directly from bash (e.g. `explorer.exe .` to open current folder in Windows Explorer).
2. Intelligent Tool Selection & Sound Engineering (CRITICAL):
   - You have full freedom to combine Bash and Python to solve any task with maximum precision.
   - For complex processing, calculations, APIs, web scraping, data parsing (JSON/YAML/XML) or reading binary/structured documents (PDF, DOCX, XLSX, images), prefer Python scripts or one-liners.
   - PROGRESS FEEDBACK & FLUSHING: In iterative loops or scripts that process items sequentially, ALWAYS emit clear progress feedback (e.g. `print(f"...", flush=True)` in Python or `echo "..."` in Bash). Never execute long iterative operations in absolute silence.
   - MODULAR STEPS VS. MONOLITHS: Whenever a complex task involves distinct logical stages (e.g., preparing environment, downloading/extracting data, processing, cleaning up), break the command into visible chained steps using `&&` or create a clean temporary script file instead of an unreadable, monolithic one-liner.
   - PYTHON VIRTUAL ENVIRONMENT (PERSISTENT BY DEFAULT):
     * A persistent, dedicated virtual environment is maintained for Terminal AI at `~/.local/share/tai/venv/bin/python` (and `pip` at `~/.local/share/tai/venv/bin/pip`).
     * Whenever Python scripts or third-party packages (pip) are required, use this persistent venv by default (e.g. `~/.local/share/tai/venv/bin/pip install -q <package> && ~/.local/share/tai/venv/bin/python -c "..."` or execute directly with `~/.local/share/tai/venv/bin/python`).
     * This avoids repeated downloads and keeps packages available across sessions.
   - EPHEMERAL VIRTUAL ENVIRONMENTS (ON DEMAND ONLY):
     * If and only if the user explicitly requests an isolated/temporary/disposable environment or if marked with "USER REQUESTED EPHEMERAL VENV", create an ephemeral venv in a clean location (e.g. `TMPVENV=$(mktemp -d) && python3 -m venv "$TMPVENV" && ...`), execute the task using `$TMPVENV/bin/python`, and guarantee the directory is deleted at the end (`rm -rf "$TMPVENV"`), leaving zero disk clutter.
   - Prefer efficient, idiomatic Linux commands. Follow symlinks (-L) for user files when searching across home and mounts.
   - When the user intent implies changing the current environment (e.g., navigating to a folder with `cd`, setting env vars with `export`), ensure `affects_parent_shell: true`.
3. Session Continuity:
   - If previous interaction history is present, interpret relative pronouns and follow-up requests ("agora faça...", "eles", "o mesmo") based on the previous command.
   - ABSOLUTE RULE: NEVER summarize, recite, or list past interactions or actions. Only output the single command line needed for the current prompt.
4. SELF-AWARENESS & SELF-MUTATION (CRITICAL):
   - You are the engine behind the CLI tool `tai` (Terminal AI).
   - If the user asks to configure, change, or check `tai` itself (e.g. "mude seu modelo para X", "mude sua api_url para Y", "mostre sua config", "mude o provedor para ollama/deepseek"):
     - To update configuration: generate `tai config set <chave> <valor>` (valid keys: provider, model, api_url, api_key, language, default_mode, max_context_tokens, stream). When switching to a local Ollama model (e.g. qwen, llama, yi), use `tai config set provider ollama && tai config set model <modelo>`. To reconfigure with guided prompts, generate `tai config interactive`.
     - To display configuration: generate `tai config show`.
     - To inspect or manage python venv: generate `tai venv status`, `tai venv clean`, `tai venv update`, or `tai venv install <pacote>`.
     - To learn or append a persistent rule to yourself: generate a bash command that appends to `~/.config/tai/rules.md`.
     - To rebuild/recompile yourself from local source: generate `tai rebuild`.
5. FEW-SHOT EXAMPLES (Follow strictly):
Example 1:
User: mostrar portas tcp abertas
Output:
%%%ss -tulpn%%%
EXPLANATION: - ss: lista conexões de rede e portas abertas

Example 2:
User: tai mostre suas configs
Output:
%%%tai config show%%%
EXPLANATION: - tai config show: exibe as configurações ativas do Terminal AI

Example 3:
User: navegue até a pasta downloads
Output:
%%%cd ~/Downloads%%%
EXPLANATION: - cd: altera o diretório atual
- ~/Downloads: pasta de downloads do usuário

Example 4:
User: abrir pasta atual no explorer do windows
Output:
%%%explorer.exe .%%%
EXPLANATION: - explorer.exe: abre o Windows Explorer nativo a partir do WSL
- .: diretório atual

6. Output Constraints:
   - The command must always be enclosed between %%% and %%%.
   - Keep the explanation concise, didactic and in language '{language}'.
   - Do not output greetings or conversational text."#
        );

        let is_anthropic = self.api_url.contains("anthropic.com");
        let headers = Self::build_headers(&self.api_url, &self.api_key);
        let url = Self::build_endpoint_url(&self.api_url);

        if is_anthropic {
            let anthropic_body = serde_json::json!({
                "model": self.model,
                "max_tokens": 1024,
                "system": system_prompt,
                "stream": on_delta.is_some(),
                "messages": [
                    { "role": "user", "content": prompt }
                ]
            });

            let response = self
                .client
                .post(&url)
                .headers(headers)
                .json(&anthropic_body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let err_text = response.text().await.unwrap_or_default();
                return Err(ProviderError::Api(status, err_text));
            }

            if let Some(cb) = on_delta {
                let mut acc = AnthropicStreamAccumulator::new();
                read_sse(response, |data| {
                    let (events, err) = acc.feed_data(data);
                    if let Some(msg) = err {
                        return Err(ProviderError::Api(reqwest::StatusCode::BAD_GATEWAY, msg));
                    }
                    events.into_iter().for_each(&mut *cb);
                    Ok(())
                })
                .await?;
                return acc.finish().ok_or(ProviderError::EmptyResponse);
            }

            let resp_json: AnthropicResponse = response.json().await?;
            let full_text: String = resp_json
                .content
                .into_iter()
                .filter(|c| c.content_type == "text")
                .map(|c| c.text)
                .collect::<Vec<_>>()
                .join("\n");

            if full_text.trim().is_empty() {
                return Err(ProviderError::EmptyResponse);
            }

            return Ok(Self::extract_suggestion(&full_text));
        }

        let tools = vec![ToolDefinition {
            tool_type: "function".to_string(),
            function: FunctionDefinition {
                name: "suggest_linux_command".to_string(),
                description: "Fornece o comando Bash exato e sua explicação técnica detalhada.".to_string(),
                parameters: FunctionParameters {
                    param_type: "object".to_string(),
                    properties: serde_json::json!({
                        "command": {
                            "type": "string",
                            "description": "O comando Bash executável completo (linha única ou encadeado com &&, |, ;). Sem markdown ou blocos de código."
                        },
                        "affects_parent_shell": {
                            "type": "boolean",
                            "description": "Defina como true se o comando alterar o ambiente do shell pai (ex: cd, export, source, unset, alias, unalias)."
                        },
                        "explanation": {
                            "type": "string",
                            "description": format!("Explicação técnica e didática das flags e ferramentas no idioma '{language}'.")
                        }
                    }),
                    required: vec!["command".to_string(), "affects_parent_shell".to_string(), "explanation".to_string()],
                },
            },
        }];

        let body = ChatCompletionRequest {
            model: self.model.clone(),
            messages: vec![
                ChatMessage {
                    role: "system".to_string(),
                    content: system_prompt,
                },
                ChatMessage {
                    role: "user".to_string(),
                    content: prompt.to_string(),
                },
            ],
            tools: Some(tools),
            tool_choice: Some(serde_json::Value::String("auto".to_string())),
            temperature: 0.1,
            stream: on_delta.is_some(),
        };

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();

            // Se for erro da Groq com tool_use_failed, resgata o comando gerado no campo failed_generation
            if status == reqwest::StatusCode::BAD_REQUEST && (err_text.contains("tool_use_failed") || err_text.contains("failed_generation")) {
                if let Ok(parsed_err) = serde_json::from_str::<serde_json::Value>(&err_text) {
                    if let Some(failed_gen) = parsed_err.get("error").and_then(|e| e.get("failed_generation")).and_then(|g| g.as_str()) {
                        let suggestion = Self::extract_suggestion(failed_gen);
                        if !suggestion.command.trim().is_empty() {
                            return Ok(suggestion);
                        }
                    }
                }
            }

            return Err(ProviderError::Api(status, err_text));
        }

        if let Some(cb) = on_delta {
            let mut acc = OpenAiStreamAccumulator::new();
            read_sse(response, |data| {
                acc.feed_data(data).into_iter().for_each(&mut *cb);
                Ok(())
            })
            .await?;
            return acc.finish().ok_or(ProviderError::EmptyResponse);
        }

        let resp_json: ChatCompletionResponse = response.json().await?;
        let choice = resp_json.choices.first().ok_or(ProviderError::EmptyResponse)?;

        // 1. Prioridade Máxima: Function Calling Estruturado (tool_calls)
        if let Some(ref tool_calls) = choice.message.tool_calls {
            for call in tool_calls {
                if call.function.name == "suggest_linux_command" {
                    if let Ok(args) = serde_json::from_str::<SuggestedCommandArguments>(&call.function.arguments) {
                        let cmd = args.command.trim().to_string();
                        let affects = args.affects_parent_shell.unwrap_or_else(|| Self::detect_affects_parent_shell(&cmd));
                        return Ok(CommandSuggestion {
                            command: cmd,
                            explanation: args.explanation.trim().to_string(),
                            affects_parent_shell: affects,
                        });
                    }
                }
            }
        }

        // 2. Fallback para modelos legados que respondem via content de texto
        let text = choice.message.content.as_deref().unwrap_or("");
        Ok(Self::extract_suggestion(text))
    }

    /// Executa uma consulta direta (system + user prompt) retornando o conteúdo textual bruto retornado pelo modelo
    pub async fn send_raw_prompt(
        &self,
        system_prompt: &str,
        user_prompt: &str,
    ) -> Result<String, ProviderError> {
        let headers = Self::build_headers(&self.api_url, &self.api_key);
        let url = Self::build_endpoint_url(&self.api_url);
        let is_anthropic = self.api_url.contains("anthropic.com");

        if is_anthropic {
            let body = serde_json::json!({
                "model": self.model,
                "max_tokens": 2048,
                "system": system_prompt,
                "messages": [
                    { "role": "user", "content": user_prompt }
                ]
            });

            let response = self
                .client
                .post(&url)
                .headers(headers)
                .json(&body)
                .send()
                .await?;

            if !response.status().is_success() {
                let status = response.status();
                let err_text = response.text().await.unwrap_or_default();
                return Err(ProviderError::Api(status, err_text));
            }

            let resp_json: AnthropicResponse = response.json().await?;
            let full_text: String = resp_json
                .content
                .into_iter()
                .filter(|c| c.content_type == "text")
                .map(|c| c.text)
                .collect::<Vec<_>>()
                .join("\n");

            if full_text.trim().is_empty() {
                return Err(ProviderError::EmptyResponse);
            }
            return Ok(full_text);
        }

        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system_prompt },
                { "role": "user", "content": user_prompt }
            ],
            "temperature": 0.1,
        });

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let err_text = response.text().await.unwrap_or_default();
            return Err(ProviderError::Api(status, err_text));
        }

        let resp_json: ChatCompletionResponse = response.json().await?;
        let choice = resp_json.choices.first().ok_or(ProviderError::EmptyResponse)?;
        let text = choice.message.content.as_deref().unwrap_or("").to_string();
        if text.trim().is_empty() {
            return Err(ProviderError::EmptyResponse);
        }
        Ok(text)
    }
}

#[async_trait]
impl LlmProvider for OpenAiCompatibleProvider {
    async fn generate_command(
        &self,
        prompt: &str,
        os_info: &str,
        language: &str,
        session_context: &str,
        custom_rules: Option<&str>,
    ) -> Result<CommandSuggestion, ProviderError> {
        self.generate_inner(prompt, os_info, language, session_context, custom_rules, None)
            .await
    }

    async fn generate_command_stream(
        &self,
        prompt: &str,
        os_info: &str,
        language: &str,
        session_context: &str,
        custom_rules: Option<&str>,
        on_delta: DeltaCallback<'_>,
    ) -> Result<CommandSuggestion, ProviderError> {
        let streamed = self
            .generate_inner(prompt, os_info, language, session_context, custom_rules, Some(on_delta))
            .await;
        match streamed {
            // Fallback: provedor rejeitou `stream` ou não devolveu nada utilizável.
            Err(ProviderError::Api(status, _))
                if status == reqwest::StatusCode::BAD_REQUEST
                    || status == reqwest::StatusCode::UNPROCESSABLE_ENTITY =>
            {
                self.generate_command(prompt, os_info, language, session_context, custom_rules).await
            }
            Err(ProviderError::EmptyResponse) => {
                self.generate_command(prompt, os_info, language, session_context, custom_rules).await
            }
            Ok(s) if s.command.trim().is_empty() => {
                self.generate_command(prompt, os_info, language, session_context, custom_rules).await
            }
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_suggestion_percent_markers() {
        let raw = "Aqui está o comando:\n%%%cd ~/Downloads%%%\nEXPLANATION: altera para a pasta downloads.";
        let res = OpenAiCompatibleProvider::extract_suggestion(raw);
        assert_eq!(res.command, "cd ~/Downloads");
        assert!(res.explanation.contains("altera para a pasta downloads"));
    }

    #[test]
    fn test_extract_suggestion_markdown_fallback() {
        let raw = "Aqui está o comando:\n```bash\ntar -czvf src.tar.gz src/\n```\nEle compacta a pasta.";
        let res = OpenAiCompatibleProvider::extract_suggestion(raw);
        assert_eq!(res.command, "tar -czvf src.tar.gz src/");
    }

    #[test]
    fn test_build_endpoint_url_various_providers() {
        assert_eq!(
            OpenAiCompatibleProvider::build_endpoint_url("https://api.deepseek.com"),
            "https://api.deepseek.com/v1/chat/completions"
        );
        assert_eq!(
            OpenAiCompatibleProvider::build_endpoint_url("https://api.openai.com/v1"),
            "https://api.openai.com/v1/chat/completions"
        );
        assert_eq!(
            OpenAiCompatibleProvider::build_endpoint_url("https://generativelanguage.googleapis.com/v1beta/openai/"),
            "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
        );
        assert_eq!(
            OpenAiCompatibleProvider::build_endpoint_url("https://open.bigmodel.cn/api/paas/v4/"),
            "https://open.bigmodel.cn/api/paas/v4/chat/completions"
        );
        assert_eq!(
            OpenAiCompatibleProvider::build_endpoint_url("https://api.anthropic.com/v1"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            OpenAiCompatibleProvider::build_endpoint_url("http://localhost:11434/v1"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_headers_anthropic_vs_bearer() {
        let anthropic_headers = OpenAiCompatibleProvider::build_headers("https://api.anthropic.com/v1", "sk-ant-test123");
        assert_eq!(anthropic_headers.get("x-api-key").unwrap(), "sk-ant-test123");
        assert_eq!(anthropic_headers.get("anthropic-version").unwrap(), "2023-06-01");
        assert!(anthropic_headers.get(AUTHORIZATION).is_none());

        let openai_headers = OpenAiCompatibleProvider::build_headers("https://api.openai.com/v1", "sk-open-test123");
        assert_eq!(openai_headers.get(AUTHORIZATION).unwrap(), "Bearer sk-open-test123");
        assert!(openai_headers.get("x-api-key").is_none());
    }
}
