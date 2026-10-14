use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::fs::{create_dir_all, File};
use std::io::{Read, Write};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Não foi possível determinar o diretório de configuração do sistema")]
    NoConfigDir,
    #[error("Erro de I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("Erro ao serializar/deserializar TOML: {0}")]
    TomlDe(#[from] toml::de::Error),
    #[error("Erro ao serializar TOML: {0}")]
    TomlSer(#[from] toml::ser::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSpec {
    pub id: &'static str,
    pub name: &'static str,
    pub default_url: &'static str,
    pub default_model: &'static str,
    pub models: &'static [&'static str],
    pub requires_api_key: bool,
}

pub const KNOWN_PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        id: "deepseek",
        name: "DeepSeek",
        default_url: "https://api.deepseek.com",
        default_model: "deepseek-chat",
        models: &["deepseek-chat", "deepseek-reasoner"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "openai",
        name: "OpenAI",
        default_url: "https://api.openai.com/v1",
        default_model: "gpt-4o-mini",
        models: &["gpt-4o", "gpt-4o-mini", "gpt-4-turbo", "o1-mini", "o3-mini"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "gemini",
        name: "Google Gemini",
        default_url: "https://generativelanguage.googleapis.com/v1beta/openai/",
        default_model: "gemini-2.0-flash",
        models: &["gemini-2.0-flash", "gemini-1.5-pro", "gemini-1.5-flash"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "anthropic",
        name: "Anthropic Claude",
        default_url: "https://api.anthropic.com/v1",
        default_model: "claude-3-5-sonnet-latest",
        models: &[
            "claude-3-5-sonnet-latest",
            "claude-3-5-haiku-latest",
            "claude-3-opus-latest",
        ],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "mistral",
        name: "Mistral AI / Codestral",
        default_url: "https://api.mistral.ai/v1",
        default_model: "codestral-latest",
        models: &["codestral-latest", "mistral-large-latest", "mistral-small-latest"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "qwen",
        name: "Alibaba Qwen (DashScope)",
        default_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1",
        default_model: "qwen2.5-coder-32b-instruct",
        models: &[
            "qwen2.5-coder-32b-instruct",
            "qwen2.5-coder-7b-instruct",
            "qwen-plus",
            "qwen-turbo",
            "qwen-max",
        ],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "zhipu",
        name: "Zhipu AI (GLM)",
        default_url: "https://open.bigmodel.cn/api/paas/v4/",
        default_model: "glm-4-flash",
        models: &["glm-4-flash", "glm-4-plus", "glm-4-air"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "moonshot",
        name: "Moonshot AI (Kimi)",
        default_url: "https://api.moonshot.cn/v1",
        default_model: "moonshot-v1-8k",
        models: &["moonshot-v1-8k", "moonshot-v1-32k", "moonshot-v1-128k"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "yi",
        name: "01.AI (Yi)",
        default_url: "https://api.01.ai/v1",
        default_model: "yi-lightning",
        models: &["yi-lightning", "yi-large", "yi-coder"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "groq",
        name: "Groq",
        default_url: "https://api.groq.com/openai/v1",
        default_model: "llama-3.3-70b-versatile",
        models: &["llama-3.3-70b-versatile", "llama-3.1-8b-instant", "mixtral-8x7b-32768"],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "openrouter",
        name: "OpenRouter",
        default_url: "https://openrouter.ai/api/v1",
        default_model: "deepseek/deepseek-chat",
        models: &[
            "deepseek/deepseek-chat",
            "openai/gpt-4o-mini",
            "anthropic/claude-3.5-sonnet",
            "meta-llama/llama-3.3-70b-instruct",
        ],
        requires_api_key: true,
    },
    ProviderSpec {
        id: "ollama",
        name: "Ollama (Local)",
        default_url: "http://localhost:11434/v1",
        default_model: "qwen2.5-coder:1.5b",
        models: &[
            "qwen2.5-coder:1.5b",
            "qwen2.5-coder:7b",
            "llama3.2:3b",
            "deepseek-r1:1.5b",
        ],
        requires_api_key: false,
    },
    ProviderSpec {
        id: "custom",
        name: "Personalizado / Inserir Outro",
        default_url: "",
        default_model: "",
        models: &[],
        requires_api_key: false,
    },
];

impl ProviderSpec {
    pub fn all() -> &'static [ProviderSpec] {
        KNOWN_PROVIDERS
    }

    pub fn find(id: &str) -> Option<&'static ProviderSpec> {
        let needle = id.trim().to_lowercase();
        let normalized = match needle.as_str() {
            "dashscope" | "tongyi" | "alibaba" => "qwen",
            "glm" | "chatglm" | "bigmodel" => "zhipu",
            "kimi" => "moonshot",
            "claude" => "anthropic",
            "google" => "gemini",
            "01ai" | "01.ai" => "yi",
            other => other,
        };
        KNOWN_PROVIDERS.iter().find(|p| p.id == normalized)
    }

    pub fn find_by_model(model: &str) -> Option<&'static ProviderSpec> {
        let m = model.trim().to_lowercase();
        for spec in KNOWN_PROVIDERS {
            if spec.models.iter().any(|known| known.to_lowercase() == m) {
                return Some(spec);
            }
        }
        if m.starts_with("gemini-") {
            return Self::find("gemini");
        }
        if m.starts_with("claude-") {
            return Self::find("anthropic");
        }
        if m.starts_with("codestral") || m.starts_with("mistral") {
            return Self::find("mistral");
        }
        if m.starts_with("glm-") {
            return Self::find("zhipu");
        }
        if m.starts_with("moonshot-") || m.starts_with("kimi") {
            return Self::find("moonshot");
        }
        if m.starts_with("yi-") && !m.contains(':') {
            return Self::find("yi");
        }
        if m.contains(':')
            || m.starts_with("qwen")
            || m.starts_with("llama")
            || m.starts_with("yi-coder")
        {
            return Self::find("ollama");
        }
        None
    }

    pub fn list_ids() -> Vec<&'static str> {
        KNOWN_PROVIDERS.iter().map(|p| p.id).collect()
    }

    pub fn is_valid_or_custom(id: &str) -> bool {
        Self::find(id).is_some()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub api_url: String,
    pub language: String,
    pub default_mode: String, // "interactive", "print", "execute"
    #[serde(default = "default_max_context_tokens")]
    pub max_context_tokens: usize,
    #[serde(default = "default_ollama_url")]
    pub ollama_url: String,
    #[serde(default = "default_stream")]
    pub stream: bool,
    #[serde(default = "default_task_steps")]
    pub task_default_steps: usize,
}

fn default_stream() -> bool {
    true
}

fn default_task_steps() -> usize {
    5
}

fn default_max_context_tokens() -> usize {
    50000
}

fn default_ollama_url() -> String {
    "http://localhost:11434/v1".to_string()
}

impl Default for Config {
    fn default() -> Self {
        Self {
            provider: "deepseek".to_string(),
            model: "deepseek-chat".to_string(),
            api_key: String::new(),
            api_url: "https://api.deepseek.com".to_string(),
            language: "pt".to_string(),
            default_mode: "interactive".to_string(),
            max_context_tokens: default_max_context_tokens(),
            ollama_url: default_ollama_url(),
            stream: default_stream(),
            task_default_steps: default_task_steps(),
        }
    }
}

impl Config {
    pub fn config_path() -> Result<PathBuf, ConfigError> {
        let proj_dirs = ProjectDirs::from("com", "terminal_ai", "tai")
            .ok_or(ConfigError::NoConfigDir)?;
        let config_dir = proj_dirs.config_dir();
        create_dir_all(config_dir)?;
        Ok(config_dir.join("config.toml"))
    }

    pub fn rules_path() -> Result<PathBuf, ConfigError> {
        let proj_dirs = ProjectDirs::from("com", "terminal_ai", "tai")
            .ok_or(ConfigError::NoConfigDir)?;
        let config_dir = proj_dirs.config_dir();
        create_dir_all(config_dir)?;
        Ok(config_dir.join("rules.md"))
    }

    pub fn load_rules() -> Option<String> {
        if let Ok(path) = Self::rules_path() {
            if path.exists() {
                if let Ok(mut file) = File::open(&path) {
                    let mut content = String::new();
                    if file.read_to_string(&mut content).is_ok() {
                        let trimmed = content.trim();
                        if !trimmed.is_empty() {
                            return Some(trimmed.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    pub fn load() -> Result<Self, ConfigError> {
        let path = Self::config_path()?;
        if !path.exists() {
            let default_cfg = Self::default();
            default_cfg.save()?;
            return Ok(default_cfg);
        }

        let mut file = File::open(&path)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents)?;

        let mut config: Config = toml::from_str(&contents)?;

        // Sobrescrita de provedor por TAI_PROVIDER
        if let Ok(provider) = std::env::var("TAI_PROVIDER") {
            if !provider.is_empty() {
                config.sync_for_provider(&provider);
            }
        }

        // Sobrescrita por variáveis de ambiente de chave de API
        if let Ok(key) = std::env::var("TAI_API_KEY") {
            if !key.is_empty() {
                config.api_key = key;
            }
        } else {
            let env_key = match config.provider.as_str() {
                "deepseek" => std::env::var("DEEPSEEK_API_KEY"),
                "openai" => std::env::var("OPENAI_API_KEY"),
                "gemini" => std::env::var("GEMINI_API_KEY"),
                "groq" => std::env::var("GROQ_API_KEY"),
                "anthropic" => std::env::var("ANTHROPIC_API_KEY"),
                "mistral" => std::env::var("MISTRAL_API_KEY"),
                "qwen" => std::env::var("DASHSCOPE_API_KEY").or_else(|_| std::env::var("QWEN_API_KEY")),
                "zhipu" => std::env::var("ZHIPU_API_KEY").or_else(|_| std::env::var("GLM_API_KEY")),
                "moonshot" => std::env::var("MOONSHOT_API_KEY").or_else(|_| std::env::var("KIMI_API_KEY")),
                "yi" => std::env::var("YI_API_KEY"),
                "openrouter" => std::env::var("OPENROUTER_API_KEY"),
                _ => std::env::var("DEEPSEEK_API_KEY"),
            };
            if let Ok(key) = env_key {
                if !key.is_empty() {
                    config.api_key = key;
                }
            }
        }

        Ok(config)
    }

    pub fn save(&self) -> Result<(), ConfigError> {
        let path = Self::config_path()?;
        let serialized = toml::to_string_pretty(self)?;

        let mut file = File::create(&path)?;
        file.write_all(serialized.as_bytes())?;

        // Garantir permissão 0600 no Linux (somente leitura/escrita do próprio usuário)
        #[cfg(unix)]
        {
            let mut perms = file.metadata()?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&path, perms)?;
        }

        Ok(())
    }

    pub fn masked_api_key(&self) -> String {
        if self.api_key.len() <= 8 {
            return "********".to_string();
        }
        let prefix = &self.api_key[..6];
        let suffix = &self.api_key[self.api_key.len() - 4..];
        format!("{prefix}...{suffix}")
    }

    /// Sincroniza provider, api_url e modelo de acordo com o modelo selecionado sem apagar credenciais
    pub fn sync_for_model(&mut self, model: &str) {
        if let Some(spec) = ProviderSpec::find_by_model(model) {
            self.provider = spec.id.to_string();
            if spec.id == "ollama" {
                self.api_url = self.ollama_url.clone();
            } else if !spec.default_url.is_empty() {
                self.api_url = spec.default_url.to_string();
            }
        }
        self.model = model.to_string();
    }

    /// Sincroniza api_url e modelo padrão de acordo com o provider selecionado sem destruir credenciais
    pub fn sync_for_provider(&mut self, provider: &str) {
        let p = provider.trim().to_lowercase();
        self.provider = p.clone();

        if p == "ollama" {
            self.api_url = self.ollama_url.clone();
            if self.model == "deepseek-chat"
                || self.model == "deepseek-reasoner"
                || self.model.starts_with("gpt-")
                || self.model.starts_with("llama-3.3-70b")
            {
                self.model = "qwen2.5-coder:1.5b".to_string();
            }
            // A chave api_key NUNCA é sobrescrita para que permaneça salva ao voltar para nuvem.
        } else if let Some(spec) = ProviderSpec::find(&p) {
            if !spec.default_url.is_empty() {
                self.api_url = spec.default_url.to_string();
            }
            if !spec.default_model.is_empty() {
                self.model = spec.default_model.to_string();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.provider, "deepseek");
        assert_eq!(cfg.model, "deepseek-chat");
        assert_eq!(cfg.language, "pt");
        assert_eq!(cfg.max_context_tokens, 50000);
        assert!(cfg.stream);
        assert_eq!(cfg.api_key, "");
        assert_eq!(cfg.masked_api_key(), "********");
    }

    #[test]
    fn test_stream_config_default_and_deserialize() {
        let toml_str = r#"
            provider = "deepseek"
            model = "deepseek-chat"
            api_key = "test"
            api_url = "https://api.deepseek.com"
            language = "pt"
            default_mode = "interactive"
        "#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert!(cfg.stream);

        let toml_str_false = r#"
            provider = "deepseek"
            model = "deepseek-chat"
            api_key = "test"
            api_url = "https://api.deepseek.com"
            language = "pt"
            default_mode = "interactive"
            stream = false
        "#;
        let cfg_false: Config = toml::from_str(toml_str_false).unwrap();
        assert!(!cfg_false.stream);
    }

    #[test]
    fn test_sync_for_model_ollama() {
        let mut cfg = Config::default();
        let original_key = cfg.api_key.clone();
        assert_eq!(cfg.provider, "deepseek");
        assert_eq!(cfg.api_url, "https://api.deepseek.com");

        cfg.sync_for_model("qwen2.5-coder:1.5b");
        assert_eq!(cfg.provider, "ollama");
        assert_eq!(cfg.model, "qwen2.5-coder:1.5b");
        assert_eq!(cfg.api_url, cfg.ollama_url);
        assert_eq!(cfg.api_key, original_key);
    }

    #[test]
    fn test_sync_for_model_deepseek() {
        let mut cfg = Config {
            provider: "ollama".to_string(),
            api_url: "http://localhost:11434/v1".to_string(),
            model: "qwen2.5-coder:1.5b".to_string(),
            ..Default::default()
        };

        cfg.sync_for_model("deepseek-chat");
        assert_eq!(cfg.provider, "deepseek");
        assert_eq!(cfg.model, "deepseek-chat");
        assert_eq!(cfg.api_url, "https://api.deepseek.com");
    }

    #[test]
    fn test_sync_for_provider_ollama() {
        let mut cfg = Config::default();
        let original_key = cfg.api_key.clone();
        cfg.sync_for_provider("ollama");
        assert_eq!(cfg.provider, "ollama");
        assert_eq!(cfg.api_url, cfg.ollama_url);
        assert_eq!(cfg.model, "qwen2.5-coder:1.5b");
        assert_eq!(cfg.api_key, original_key);
    }

    #[test]
    fn test_sync_for_provider_deepseek() {
        let mut cfg = Config::default();
        let original_key = cfg.api_key.clone();
        cfg.sync_for_provider("ollama");
        cfg.sync_for_provider("deepseek");
        assert_eq!(cfg.provider, "deepseek");
        assert_eq!(cfg.api_url, "https://api.deepseek.com");
        assert_eq!(cfg.model, "deepseek-chat");
        assert_eq!(cfg.api_key, original_key);
    }

    #[test]
    fn test_provider_spec_catalog() {
        assert!(ProviderSpec::find("deepseek").is_some());
        assert!(ProviderSpec::find("openai").is_some());
        assert!(ProviderSpec::find("gemini").is_some());
        assert!(ProviderSpec::find("google").is_some());
        assert!(ProviderSpec::find("anthropic").is_some());
        assert!(ProviderSpec::find("claude").is_some());
        assert!(ProviderSpec::find("mistral").is_some());
        assert!(ProviderSpec::find("qwen").is_some());
        assert!(ProviderSpec::find("dashscope").is_some());
        assert!(ProviderSpec::find("zhipu").is_some());
        assert!(ProviderSpec::find("glm").is_some());
        assert!(ProviderSpec::find("moonshot").is_some());
        assert!(ProviderSpec::find("kimi").is_some());
        assert!(ProviderSpec::find("yi").is_some());
        assert!(ProviderSpec::find("groq").is_some());
        assert!(ProviderSpec::find("openrouter").is_some());
        assert!(ProviderSpec::find("ollama").is_some());
        assert!(ProviderSpec::find("custom").is_some());
        assert!(ProviderSpec::find("inexistente").is_none());

        assert!(ProviderSpec::is_valid_or_custom("deepseek"));
        assert!(ProviderSpec::is_valid_or_custom("gemini"));
        assert!(ProviderSpec::is_valid_or_custom("qwen"));
        assert!(ProviderSpec::is_valid_or_custom("custom"));
        assert!(!ProviderSpec::is_valid_or_custom("invalid_xyz"));

        let deepseek = ProviderSpec::find("deepseek").unwrap();
        assert_eq!(deepseek.default_url, "https://api.deepseek.com");
        assert_eq!(deepseek.default_model, "deepseek-chat");
        assert!(deepseek.requires_api_key);

        let gemini = ProviderSpec::find("gemini").unwrap();
        assert_eq!(gemini.default_url, "https://generativelanguage.googleapis.com/v1beta/openai/");
        assert_eq!(gemini.default_model, "gemini-2.0-flash");

        let ollama = ProviderSpec::find("ollama").unwrap();
        assert_eq!(ollama.default_url, "http://localhost:11434/v1");
        assert!(!ollama.requires_api_key);

        // Model lookup
        assert_eq!(ProviderSpec::find_by_model("deepseek-chat").unwrap().id, "deepseek");
        assert_eq!(ProviderSpec::find_by_model("gpt-4o").unwrap().id, "openai");
        assert_eq!(ProviderSpec::find_by_model("gemini-2.0-flash").unwrap().id, "gemini");
        assert_eq!(ProviderSpec::find_by_model("claude-3-5-sonnet-latest").unwrap().id, "anthropic");
        assert_eq!(ProviderSpec::find_by_model("codestral-latest").unwrap().id, "mistral");
        assert_eq!(ProviderSpec::find_by_model("qwen2.5-coder-32b-instruct").unwrap().id, "qwen");
        assert_eq!(ProviderSpec::find_by_model("glm-4-flash").unwrap().id, "zhipu");
        assert_eq!(ProviderSpec::find_by_model("moonshot-v1-8k").unwrap().id, "moonshot");
        assert_eq!(ProviderSpec::find_by_model("yi-lightning").unwrap().id, "yi");
        assert_eq!(ProviderSpec::find_by_model("llama-3.3-70b-versatile").unwrap().id, "groq");
        assert_eq!(ProviderSpec::find_by_model("qwen2.5-coder:1.5b").unwrap().id, "ollama");
    }

    #[test]
    fn test_custom_provider_persisted() {
        let cfg = Config {
            provider: "custom".to_string(),
            api_url: "https://custom-ai.company.org/v1".to_string(),
            api_key: "sk-custom-secret-key-1234".to_string(),
            model: "custom-model-v1".to_string(),
            ..Default::default()
        };

        assert_eq!(cfg.provider, "custom");
        assert_eq!(cfg.api_url, "https://custom-ai.company.org/v1");
        assert_eq!(cfg.api_key, "sk-custom-secret-key-1234");
        assert_eq!(cfg.model, "custom-model-v1");
    }

    #[test]
    fn test_ollama_url_persistence() {
        let mut cfg = Config {
            ollama_url: "http://192.168.1.100:11434/v1".to_string(),
            ..Default::default()
        };
        cfg.sync_for_provider("ollama");
        assert_eq!(cfg.api_url, "http://192.168.1.100:11434/v1");
    }

    #[test]
    fn test_sync_for_new_providers() {
        let mut cfg = Config::default();
        let key = cfg.api_key.clone();

        cfg.sync_for_provider("gemini");
        assert_eq!(cfg.provider, "gemini");
        assert_eq!(cfg.model, "gemini-2.0-flash");
        assert_eq!(cfg.api_url, "https://generativelanguage.googleapis.com/v1beta/openai/");
        assert_eq!(cfg.api_key, key);

        cfg.sync_for_provider("anthropic");
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.model, "claude-3-5-sonnet-latest");
        assert_eq!(cfg.api_url, "https://api.anthropic.com/v1");

        cfg.sync_for_provider("qwen");
        assert_eq!(cfg.provider, "qwen");
        assert_eq!(cfg.model, "qwen2.5-coder-32b-instruct");
        assert_eq!(cfg.api_url, "https://dashscope-intl.aliyuncs.com/compatible-mode/v1");

        // Sync via model name
        cfg.sync_for_model("claude-3-haiku-20240307");
        assert_eq!(cfg.provider, "anthropic");
        assert_eq!(cfg.api_url, "https://api.anthropic.com/v1");
        assert_eq!(cfg.model, "claude-3-haiku-20240307");
    }
}
