use clap::{Args, Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "tai",
    bin_name = "tai",
    version = "0.1.0",
    about = "Terminal AI: Seu assistente CLI de alta performance para comandos Linux",
    long_about = "Traduz comandos em linguagem natural para comandos Bash/Linux com explicação prévia e confirmação interativa."
)]
pub struct Cli {
    /// O comando ou ação que você deseja realizar em linguagem natural
    #[arg(num_args = 0..)]
    pub prompt: Vec<String>,

    /// Executa o comando diretamente sem confirmação humana (Modo Inseguro)
    #[arg(short = 'y', long = "yes", alias = "danger", default_value_t = false)]
    pub yes: bool,

    /// Apenas imprime o comando puro no stdout (Dry-run para pipes e scripts)
    #[arg(short = 'p', long = "print", alias = "dry-run", default_value_t = false)]
    pub print: bool,

    /// Copia o comando diretamente para a área de transferência do Linux
    #[arg(short = 'c', long = "copy", default_value_t = false)]
    pub copy: bool,

    /// Modelo de IA a ser utilizado para esta execução (ex: deepseek-chat, yi-coder:1.5b)
    #[arg(short = 'm', long = "model")]
    pub model: Option<String>,

    /// Provedor a ser utilizado (ex: deepseek, openai, ollama)
    #[arg(long = "provider")]
    pub provider: Option<String>,

    /// Chave de API temporária para esta requisição
    #[arg(short = 'k', long = "api-key")]
    pub api_key: Option<String>,

    /// URL base da API
    #[arg(short = 'u', long = "api-url")]
    pub api_url: Option<String>,

    /// Idioma da explicação e mensagens (ex: pt, en)
    #[arg(long = "lang")]
    pub lang: Option<String>,

    /// Modo silencioso: omite explicações didáticas e animações
    #[arg(short = 'q', long = "quiet", default_value_t = false)]
    pub quiet: bool,

    /// Inicia uma nova sessão limpa, ignorando o contexto da chamada anterior
    #[arg(short = 'n', long = "new", default_value_t = false)]
    pub new_session: bool,

    /// Arquivo temporário onde comandos que afetam o shell pai (cd, export, source) serão gravados para execução pelo hook
    #[arg(long = "shell-hook", hide = true)]
    pub shell_hook: Option<String>,

    /// Força o uso de uma venv Python temporária/efêmera em vez da venv persistente padrão
    #[arg(long = "temp-venv", alias = "ephemeral-venv", default_value_t = false)]
    pub temp_venv: bool,

    /// Desativa o streaming em tempo real da resposta da IA
    #[arg(long = "no-stream", default_value_t = false)]
    pub no_stream: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

impl Cli {
    pub fn get_prompt_text(&self) -> String {
        self.prompt.join(" ").trim().to_string()
    }

    /// Lê dados passados via pipe Unix (stdin) com limite de segurança (~50KB)
    pub fn read_piped_stdin() -> Option<String> {
        use std::io::{self, IsTerminal, Read};
        if io::stdin().is_terminal() {
            return None;
        }

        let mut buffer = Vec::new();
        // Limita a leitura a 64 KB para evitar consumo excessivo de memória e tokens
        let mut handle = io::stdin().take(65_536);
        if handle.read_to_end(&mut buffer).is_ok() && !buffer.is_empty() {
            let s = String::from_utf8_lossy(&buffer).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
        None
    }
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Gerencia configurações do Terminal AI
    Config(ConfigArgs),
    /// Limpa o histórico da sessão recente
    Clear,
    /// Exibe o histórico de passos e execuções recentes
    Log(LogArgs),
    /// Verifica e atualiza o Terminal AI para a versão mais recente
    Update,
    /// Recompila e reinstala o Terminal AI a partir do código fonte local
    Rebuild,
    /// Gerencia o ambiente virtual Python persistente do Terminal AI
    Venv(VenvArgs),
    /// Executa um objetivo complexo em loop autônomo com múltiplos passos e checkpoint
    Task(TaskArgs),
}

#[derive(Args, Debug, Clone)]
pub struct TaskArgs {
    /// O objetivo a ser executado em loop autônomo
    #[arg(num_args = 1..)]
    pub goal: Vec<String>,

    /// Número máximo inicial de passos antes de pedir extensão (padrão: 5)
    #[arg(long = "max-steps", default_value_t = 5)]
    pub max_steps: usize,

    /// Executa os passos automaticamente sem confirmação intermediária (Modo Autônomo)
    #[arg(short = 'y', long = "yes", default_value_t = false)]
    pub yes: bool,
}

impl TaskArgs {
    pub fn get_goal_text(&self) -> String {
        self.goal.join(" ").trim().to_string()
    }
}

#[derive(Args, Debug, Clone)]
pub struct LogArgs {
    /// Quantidade de registros recentes para exibir
    #[arg(short = 'n', long = "limit", default_value_t = 10)]
    pub limit: usize,

    /// Exibe apenas o caminho do arquivo de log
    #[arg(long = "path", default_value_t = false)]
    pub path_only: bool,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub action: Option<ConfigAction>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Mostra as configurações ativas atuais
    Show,
    /// Define um valor de configuração (ex: tai config set model deepseek-chat)
    Set {
        /// Chave a ser configurada (provider, model, api_key, api_url, language, default_mode)
        key: String,
        /// Novo valor da chave
        value: String,
    },
    /// Configura provedor, modelo e chave de API de uma só vez (ex: tai config use groq openai/gpt-oss-20b gsk_...)
    Use {
        /// Provedor (ex: gemini, deepseek, groq, openai, ollama)
        provider: String,
        /// Modelo de IA (ex: gemini-3.5-flash-lite, deepseek-chat)
        model: Option<String>,
        /// Chave de API (opcional para Ollama local)
        api_key: Option<String>,
    },
    /// Inicia o assistente interativo de configuração
    Interactive,
}

#[derive(Args, Debug, Clone)]
pub struct VenvArgs {
    #[command(subcommand)]
    pub action: Option<VenvAction>,

    /// Apenas imprime o caminho absoluto da venv persistente ou interpretador
    #[arg(long = "path", default_value_t = false)]
    pub path_only: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum VenvAction {
    /// Mostra o status, caminho, versão do Python e pacotes acumulados na venv
    Status,
    /// Limpa e recria completamente o ambiente virtual da venv
    Clean,
    /// Atualiza em massa todos os pacotes instalados e ferramentas base na venv
    Update,
    /// Instala um ou mais pacotes adicionais na venv persistente
    Install {
        /// Nomes dos pacotes a serem instalados
        #[arg(required = true, num_args = 1..)]
        packages: Vec<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_prompt_and_flags() {
        let args = vec!["tai", "listar", "arquivos", "grandes", "-y"];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.get_prompt_text(), "listar arquivos grandes");
        assert!(cli.yes);
        assert!(!cli.print);
        assert!(!cli.temp_venv);
    }

    #[test]
    fn test_parse_temp_venv_flag() {
        let args = vec!["tai", "--temp-venv", "executar", "script.py"];
        let cli = Cli::parse_from(args);
        assert!(cli.temp_venv);
        assert_eq!(cli.get_prompt_text(), "executar script.py");

        let args_alias = vec!["tai", "--ephemeral-venv", "teste"];
        let cli_alias = Cli::parse_from(args_alias);
        assert!(cli_alias.temp_venv);
    }

    #[test]
    fn test_parse_print_and_model_flags() {
        let args = vec!["tai", "-p", "-m", "deepseek-chat", "mostrar", "disco"];
        let cli = Cli::parse_from(args);
        assert_eq!(cli.get_prompt_text(), "mostrar disco");
        assert!(cli.print);
        assert_eq!(cli.model.as_deref(), Some("deepseek-chat"));
    }

    #[test]
    fn test_parse_config_subcommand() {
        let args = vec!["tai", "config", "show"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Some(Commands::Config(ConfigArgs {
                action: Some(ConfigAction::Show),
            })) => {}
            _ => panic!("Esperado subcomando config show"),
        }
    }

    #[test]
    fn test_parse_log_subcommand() {
        let args = vec!["tai", "log", "-n", "20", "--path"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Some(Commands::Log(log_args)) => {
                assert_eq!(log_args.limit, 20);
                assert!(log_args.path_only);
            }
            _ => panic!("Esperado subcomando log"),
        }
    }

    #[test]
    fn test_parse_venv_subcommands() {
        let args = vec!["tai", "venv", "status"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Some(Commands::Venv(VenvArgs {
                action: Some(VenvAction::Status),
                path_only: false,
            })) => {}
            _ => panic!("Esperado tai venv status"),
        }

        let args_clean = vec!["tai", "venv", "clean"];
        let cli_clean = Cli::parse_from(args_clean);
        match cli_clean.command {
            Some(Commands::Venv(VenvArgs {
                action: Some(VenvAction::Clean),
                ..
            })) => {}
            _ => panic!("Esperado tai venv clean"),
        }

        let args_update = vec!["tai", "venv", "update"];
        let cli_update = Cli::parse_from(args_update);
        match cli_update.command {
            Some(Commands::Venv(VenvArgs {
                action: Some(VenvAction::Update),
                ..
            })) => {}
            _ => panic!("Esperado tai venv update"),
        }

        let args_path = vec!["tai", "venv", "--path"];
        let cli_path = Cli::parse_from(args_path);
        match cli_path.command {
            Some(Commands::Venv(VenvArgs { path_only: true, .. })) => {}
            _ => panic!("Esperado tai venv --path"),
        }
    }

    #[test]
    fn test_parse_no_stream_flag() {
        let args = vec!["tai", "--no-stream", "listar", "arquivos"];
        let cli = Cli::parse_from(args);
        assert!(cli.no_stream);
        assert_eq!(cli.get_prompt_text(), "listar arquivos");
    }

    #[test]
    fn test_parse_task_subcommand() {
        let args = vec!["tai", "task", "criar", "projeto", "rust", "--max-steps", "8", "-y"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Some(Commands::Task(task_args)) => {
                assert_eq!(task_args.get_goal_text(), "criar projeto rust");
                assert_eq!(task_args.max_steps, 8);
                assert!(task_args.yes);
            }
            _ => panic!("Esperado subcomando task"),
        }
    }

    #[test]
    fn test_parse_config_use_subcommand() {
        let args = vec!["tai", "config", "use", "groq", "openai/gpt-oss-20b", "gsk_test123"];
        let cli = Cli::parse_from(args);
        match cli.command {
            Some(Commands::Config(ConfigArgs {
                action: Some(ConfigAction::Use { provider, model, api_key }),
            })) => {
                assert_eq!(provider, "groq");
                assert_eq!(model.as_deref(), Some("openai/gpt-oss-20b"));
                assert_eq!(api_key.as_deref(), Some("gsk_test123"));
            }
            _ => panic!("Esperado subcomando config use"),
        }
    }
}
