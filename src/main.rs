pub mod cli;
pub mod config;
pub mod executor;
pub mod i18n;
pub mod providers;
pub mod safety;
pub mod session;
pub mod task;
pub mod updater;
pub mod venv;

use clap::Parser;
use cli::{Cli, Commands, ConfigAction, ConfigArgs, VenvAction};
use colored::Colorize;
use config::{Config, ProviderSpec};
use venv::VenvManager;
use executor::CommandExecutor;
use i18n::OsContext;
use indicatif::{ProgressBar, ProgressStyle};
use inquire::{Select, Text};
use providers::{format_latency, LlmProvider, OpenAiCompatibleProvider, StreamEvent};
use safety::{DangerLevel, SafetyAnalyzer};
use std::io::{stderr, stdout, IsTerminal, Write};
use std::time::Duration;
use updater::AutoUpdater;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let mut config = Config::load().unwrap_or_default();

    // Tratamento de subcomandos (ex: tai config / tai update)
    if let Some(ref cmd) = cli.command {
        match cmd {
            Commands::Update => {
                AutoUpdater::check_and_update().await?;
                return Ok(());
            }
            Commands::Rebuild => {
                AutoUpdater::rebuild_local()?;
                return Ok(());
            }
            Commands::Clear => {
                session::SessionState::clear();
                println!("{}", "Histórico de sessão limpo com sucesso!".green());
                return Ok(());
            }
            Commands::Log(ref log_args) => {
                if log_args.path_only {
                    if let Some(path) = session::SessionState::steps_log_path() {
                        println!("{}", path.display());
                    }
                } else {
                    session::SessionState::print_recent_logs(log_args.limit);
                }
                return Ok(());
            }
            Commands::Venv(ref venv_args) => {
                let venv_mgr = VenvManager::new()?;
                if venv_args.path_only {
                    println!("{}", venv_mgr.python_path().display());
                    return Ok(());
                }

                match venv_args.action {
                    Some(VenvAction::Clean) => {
                        let pb = ProgressBar::new_spinner();
                        pb.set_style(
                            ProgressStyle::default_spinner()
                                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                                .template("{spinner:.green} {msg}")
                                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                        );
                        pb.set_message("Limpando e recriando ambiente virtual persistente...");
                        pb.enable_steady_tick(Duration::from_millis(80));

                        let start = std::time::Instant::now();
                        venv_mgr.clean()?;
                        pb.finish_and_clear();

                        println!(
                            "{} {}",
                            "Ambiente virtual limpo e recriado com sucesso!".green().bold(),
                            format!("[⏱ {:.2}s]", start.elapsed().as_secs_f64()).dimmed()
                        );
                        println!("  Caminho: {}", venv_mgr.venv_path().display().to_string().cyan());
                        return Ok(());
                    }
                    Some(VenvAction::Update) => {
                        let pb = ProgressBar::new_spinner();
                        pb.set_style(
                            ProgressStyle::default_spinner()
                                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                                .template("{spinner:.green} {msg}")
                                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                        );
                        pb.set_message("Atualizando pacotes da venv em massa...");
                        pb.enable_steady_tick(Duration::from_millis(80));

                        let start = std::time::Instant::now();
                        let res = venv_mgr.update()?;
                        pb.finish_and_clear();

                        println!(
                            "{} {}",
                            "Pacotes da venv atualizados com sucesso!".green().bold(),
                            format!("[⏱ {:.2}s]", start.elapsed().as_secs_f64()).dimmed()
                        );
                        println!("  Pacotes verificados/atualizados: {}", res.updated_packages.join(", ").cyan());
                        return Ok(());
                    }
                    Some(VenvAction::Install { ref packages }) => {
                        let pb = ProgressBar::new_spinner();
                        pb.set_style(
                            ProgressStyle::default_spinner()
                                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                                .template("{spinner:.green} {msg}")
                                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                        );
                        pb.set_message(format!("Instalando pacote(s): {}...", packages.join(" ")));
                        pb.enable_steady_tick(Duration::from_millis(80));

                        let pkg_refs: Vec<&str> = packages.iter().map(|s| s.as_str()).collect();
                        let start = std::time::Instant::now();
                        let out = venv_mgr.install_packages(&pkg_refs)?;
                        pb.finish_and_clear();

                        println!(
                            "{} {}",
                            "Pacote(s) instalado(s) com sucesso!".green().bold(),
                            format!("[⏱ {:.2}s]", start.elapsed().as_secs_f64()).dimmed()
                        );
                        if !out.trim().is_empty() {
                            for line in out.lines().take(5) {
                                println!("  {}", line.dimmed());
                            }
                        }
                        return Ok(());
                    }
                    Some(VenvAction::Status) | None => {
                        println!("{}", "════════════════════════════════════════════════════════════".cyan());
                        println!("{}", " AMBIENTE VIRTUAL PYTHON PERSISTENTE (TAI VENV)".bold().cyan());
                        println!("{}\n", "════════════════════════════════════════════════════════════".cyan());

                        let exists = venv_mgr.exists();
                        let status_badge = if exists {
                            "Ativo (Persistente)".green().bold()
                        } else {
                            "Não inicializado (será criado no primeiro comando)".yellow()
                        };

                        println!("  Status:           {}", status_badge);
                        println!("  Diretório Venv:   {}", venv_mgr.venv_path().display().to_string().cyan());
                        println!("  Interpretador:    {}", venv_mgr.python_path().display().to_string().dimmed());

                        if exists {
                            if let Ok(ver) = venv_mgr.get_python_version() {
                                println!("  Versão Python:    {}", ver.green());
                            }
                            let bytes = venv_mgr.get_disk_size();
                            println!("  Tamanho em Disco: {}", VenvManager::format_disk_size(bytes).yellow());

                            match venv_mgr.list_installed_packages() {
                                Ok(pkgs) => {
                                    println!("\n  {} ({}):", "Pacotes Acumulados".bold().white(), pkgs.len().to_string().cyan());
                                    if pkgs.is_empty() {
                                        println!("    (nenhum pacote instalado)");
                                    } else {
                                        for (name, version) in pkgs {
                                            println!("    • {:<20} {}", name.green(), version.dimmed());
                                        }
                                    }
                                }
                                Err(e) => {
                                    println!("  Erro ao listar pacotes: {}", e.to_string().red());
                                }
                            }
                        } else {
                            println!("\n  {}", "Dica: Execute 'tai venv clean' ou qualquer comando Python via 'tai' para inicializá-lo.".dimmed());
                        }

                        println!("\n{}", "────────────────────────────────────────────────────────────".dimmed());
                        return Ok(());
                    }
                }
            }
            Commands::Config(ConfigArgs { ref action }) => match action {
            Some(ConfigAction::Show) => {
                println!("{}", "Configurações do Terminal AI:".bold().cyan());
                println!("  Provedor:     {}", config.provider);
                println!("  Modelo:       {}", config.model);
                println!("  API URL:      {}", config.api_url);
                println!("  API Key:      {}", config.masked_api_key());
                println!("  Ollama URL:   {}", config.ollama_url);
                println!("  Idioma:       {}", config.language);
                println!("  Modo Padrão:  {}", config.default_mode);
                println!("  Max Contexto: {} tokens", config.max_context_tokens);
                println!("  Streaming:    {}", if config.stream { "ativado".green() } else { "desativado".yellow() });
                println!("  Passos Tarefa:{} passos", config.task_default_steps.to_string().cyan());
                return Ok(());
            }
            Some(ConfigAction::Set { key, value }) => {
                match key.as_str() {
                    "provider" => {
                        let lower_val = value.trim().to_lowercase();
                        if !ProviderSpec::is_valid_or_custom(&lower_val) {
                            eprintln!("{}", format!("Provedor não reconhecido: '{value}'").red().bold());
                            eprintln!("\nProvedores suportados na lista fixa:");
                            for spec in ProviderSpec::all() {
                                eprintln!("  • {:<12} ({})", spec.id.green().bold(), spec.name);
                            }
                            eprintln!("\nPara provedores arbitrários compatíveis com OpenAI, use '{}'.", "custom".cyan().bold());
                            eprintln!("Exemplo:");
                            eprintln!("  tai config set provider custom");
                            eprintln!("  tai config set api_url https://seu-endpoint.com/v1");
                            eprintln!("  tai config set model seu-modelo");
                            return Ok(());
                        }

                        config.sync_for_provider(value);
                        if let Some(spec) = ProviderSpec::find(&config.provider) {
                            if spec.id != "custom" {
                                println!("{}", format!("Provedor alterado para '{}'. Presets de 'api_url' ({}) e 'model' ({}) sincronizados.", spec.name, spec.default_url, spec.default_model).cyan());
                            } else {
                                println!("{}", "Provedor alterado para 'custom'. Defina 'api_url' e 'model' conforme necessário.".cyan());
                            }
                        }
                    }
                    "model" => {
                        let prev_provider = config.provider.clone();
                        config.sync_for_model(value);
                        if config.provider == "ollama" && prev_provider != "ollama" {
                            println!("{}", "Provedor e API URL sincronizados automaticamente para Ollama local (http://localhost:11434/v1).".cyan());
                        } else if config.provider != prev_provider {
                            println!("{}", format!("Provedor e API URL sincronizados automaticamente para '{}' ({}).", config.provider, config.api_url).cyan());
                        }
                    }
                    "api_key" => config.api_key = value.to_string(),
                    "api_url" => {
                        let mut url = value.to_string();
                        // Garante que URLs do Ollama terminadas na porta 11434 tenham /v1 para compatibilidade
                        if url == "http://localhost:11434" || url == "http://127.0.0.1:11434" {
                            url = format!("{url}/v1");
                        }
                        config.api_url = url;
                    }
                    "ollama_url" | "ollama_endpoint" => {
                        let mut url = value.to_string();
                        if url == "http://localhost:11434" || url == "http://127.0.0.1:11434" {
                            url = format!("{url}/v1");
                        }
                        config.ollama_url = url.clone();
                        if config.provider == "ollama" {
                            config.api_url = url;
                        }
                    }
                    "language" | "lang" => config.language = value.to_string(),
                    "default_mode" => config.default_mode = value.to_string(),
                    "max_context_tokens" | "context_tokens" | "max_tokens" => {
                        match value.parse::<usize>() {
                            Ok(tokens) if tokens > 0 => {
                                config.max_context_tokens = tokens;
                            }
                            _ => {
                                eprintln!("{}", "O valor de max_context_tokens deve ser um número inteiro positivo.".red());
                                return Ok(());
                            }
                        }
                    }
                    "stream" | "streaming" => {
                        match value.trim().to_lowercase().as_str() {
                            "true" | "1" | "yes" | "on" | "sim" | "ativado" => config.stream = true,
                            "false" | "0" | "no" | "off" | "nao" | "não" | "desativado" => config.stream = false,
                            _ => {
                                eprintln!("{}", "O valor de stream deve ser 'true' ou 'false'.".red());
                                return Ok(());
                            }
                        }
                    }
                    "task_default_steps" | "task_steps" | "max_steps" => {
                        match value.parse::<usize>() {
                            Ok(steps) if steps > 0 => {
                                config.task_default_steps = steps;
                            }
                            _ => {
                                eprintln!("{}", "O valor de task_default_steps deve ser um número inteiro positivo.".red());
                                return Ok(());
                            }
                        }
                    }
                    _ => {
                        eprintln!("{}", format!("Chave desconhecida: {key}").red());
                        return Ok(());
                    }
                }
                config.save()?;
                println!("{}", format!("Configuração '{key}' atualizada com sucesso!").green());
                return Ok(());
            }
            Some(ConfigAction::Use { provider, model, api_key }) => {
                let lower_prov = provider.trim().to_lowercase();
                if !ProviderSpec::is_valid_or_custom(&lower_prov) {
                    eprintln!("{}", format!("Provedor não reconhecido: '{provider}'").red().bold());
                    return Ok(());
                }

                config.sync_for_provider(&lower_prov);

                if let Some(m) = model {
                    config.sync_for_model(m);
                }

                if let Some(k) = api_key {
                    config.api_key = k.trim().to_string();
                }

                config.save()?;
                println!("{}", "════════════════════════════════════════════════════════════".cyan());
                println!("{}", " ✓ PROVEDOR, MODELO E CHAVE ATUALIZADOS!".bold().green());
                println!("{}", "════════════════════════════════════════════════════════════".cyan());
                println!("  Provedor: {}", config.provider.green().bold());
                println!("  Modelo:   {}", config.model.cyan());
                println!("  Endpoint: {}", config.api_url.dimmed());
                println!("  API Key:  {}", config.masked_api_key().yellow());
                println!("{}", "────────────────────────────────────────────────────────────\n".dimmed());
                return Ok(());
            }
            Some(ConfigAction::Interactive) | None => {
                run_interactive_config(&mut config).await?;
                return Ok(());
            }
        },
        Commands::Task(ref task_args) => {
            let goal = task_args.get_goal_text();
            if goal.is_empty() {
                eprintln!("{}", "Erro: Nenhum objetivo fornecido para o comando 'tai task'.".red().bold());
                std::process::exit(1);
            }

            let max_steps = if task_args.max_steps > 0 {
                task_args.max_steps
            } else {
                config.task_default_steps
            };

            println!("{}", "════════════════════════════════════════════════════════════".cyan());
            println!("{}", " 🚀 MODO DE TAREFA EM LOOP AUTÔNOMO (TAI TASK)".bold().cyan());
            println!("{}", "════════════════════════════════════════════════════════════".cyan());
            println!("  {} {}", "Objetivo:".bold().white(), goal.bold().green());
            println!("  {} {} passos", "Limite Inicial:".bold().white(), max_steps.to_string().yellow());
            println!("  {} {}", "Modo:".bold().white(), if task_args.yes || cli.yes { "Totalmente Autônomo (-y)".red().bold() } else { "Supervisionado (confirmação por passo)".cyan() });
            println!("{}\n", "────────────────────────────────────────────────────────────".dimmed());

            let os_info = OsContext::get_info();
            let provider = OpenAiCompatibleProvider::new(
                config.api_url.clone(),
                config.api_key.clone(),
                config.model.clone(),
            );
            let custom_rules = Config::load_rules();

            let mut runner = task::TaskRunner::new(goal, max_steps);
            let auto_mode = task_args.yes || cli.yes;

            match runner.run(&provider, &os_info, &config.language, custom_rules.as_deref(), auto_mode).await {
                Ok(res) => {
                    match res {
                        task::TaskRunResult::Completed { total_steps, .. } => {
                            println!("{}", format!("✓ Tarefa concluída com sucesso em {total_steps} passo(s)!").bold().green());
                        }
                        task::TaskRunResult::MaxStepsReached { total_steps, .. } => {
                            println!("{}", format!("⚠️ Tarefa finalizada no limite de {total_steps} passos.").yellow());
                        }
                        task::TaskRunResult::Blocked { reason, .. } => {
                            println!("{}", format!("⚠️ Tarefa bloqueada: {reason}").yellow());
                        }
                        task::TaskRunResult::SafetyAbort { command, danger_reason } => {
                            eprintln!("{}", format!("🚨 Tarefa abortada por segurança contra comando catastrófico `{command}`: {danger_reason}").red().bold());
                            std::process::exit(1);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{} {e}", "Erro durante execução da tarefa:".red().bold());
                    std::process::exit(1);
                }
            }
            return Ok(());
        }
        }
    }

    // Sobrescrita por flags dinâmicas
    if let Some(ref p) = cli.provider {
        config.sync_for_provider(p);
    }
    if let Some(ref m) = cli.model {
        config.sync_for_model(m);
    }
    if let Some(ref k) = cli.api_key {
        config.api_key = k.clone();
    }
    if let Some(ref u) = cli.api_url {
        config.api_url = u.clone();
    }
    if let Some(ref l) = cli.lang {
        config.language = l.clone();
    }
    if cli.no_stream {
        config.stream = false;
    }

    let user_arg_prompt = cli.get_prompt_text();
    let piped_input = Cli::read_piped_stdin();

    if user_arg_prompt.is_empty() && piped_input.is_none() {
        println!("{}", "Uso: tai <o que você quer fazer no Linux>".yellow().bold());
        println!("Exemplo: tai 'listar arquivos maiores que 50MB ordenados por tamanho'");
        println!("Exemplo com pipe: cat erro.log | tai 'analise o erro e gere o comando de correcao'");
        println!("Para opções, use: tai --help");
        return Ok(());
    }

    let prompt = match (piped_input, user_arg_prompt.is_empty()) {
        (Some(pipe), false) => format!("DADOS RECEBIDOS VIA STDIN / PIPE:\n{pipe}\n\nINSTRUÇÃO DO USUÁRIO:\n{user_arg_prompt}"),
        (Some(pipe), true) => format!("DADOS RECEBIDOS VIA STDIN / PIPE:\n{pipe}\n\nINSTRUÇÃO DO USUÁRIO:\nAnalise estes dados do terminal e forneça o próximo comando Bash apropriado para prosseguir ou diagnosticar."),
        (None, _) => user_arg_prompt,
    };

    let os_info = OsContext::get_info();

    // Gerenciamento de sessão: carrega histórico ativo a menos que -n / --new tenha sido passado
    let mut session = if cli.new_session {
        session::SessionState::default()
    } else {
        session::SessionState::load_active()
    };
    let mut session_context = session.format_for_prompt();

    // Detecção dinâmica de stack local (<2ms)
    if let Some(stack) = session::SessionState::detect_local_stack() {
        session_context.push_str(&format!("\nPROJECT STACK DETECTED IN CURRENT DIRECTORY: {stack}. Align commands and tools to this ecosystem when relevant.\n"));
    }

    // Detecção dinâmica de ambiente WSL (<2ms)
    if let Some(wsl_ctx) = OsContext::wsl_prompt_context() {
        session_context.push_str(&wsl_ctx);
    }

    if cli.temp_venv {
        session_context.push_str("\nUSER REQUESTED EPHEMERAL VENV: Use a temporary virtual environment (TMPVENV=$(mktemp -d) && python3 -m venv \"$TMPVENV\" && ... && rm -rf \"$TMPVENV\") instead of the persistent venv.\n");
    }

    let is_tty = stdout().is_terminal() && stderr().is_terminal();
    let can_stream_visually = config.stream && !cli.print && !cli.quiet && is_tty;

    // Spinner elegante durante consulta à LLM
    let spinner = if !cli.quiet && !cli.print {
        let pb = ProgressBar::new_spinner();
        pb.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.green} {msg} [{elapsed}]")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        pb.set_message("Consultando IA...");
        pb.enable_steady_tick(Duration::from_millis(80));
        Some(pb)
    } else {
        None
    };

    let provider = OpenAiCompatibleProvider::new(
        config.api_url.clone(),
        config.api_key.clone(),
        config.model.clone(),
    );

    let custom_rules = Config::load_rules();

    let mut preview_started = false;
    let mut preview_lines = 0usize;
    let spinner_handle = spinner.clone();

    let mut delta_handler = |event: StreamEvent| {
        if !can_stream_visually {
            return;
        }
        match event {
            StreamEvent::CommandDelta(delta) => {
                if !preview_started {
                    if let Some(ref pb) = spinner_handle {
                        pb.finish_and_clear();
                    }
                    eprint!("{} ", "Gerando:".cyan().bold());
                    preview_started = true;
                }
                preview_lines += delta.matches('\n').count();
                eprint!("{}", delta.green());
                let _ = stderr().flush();
            }
            StreamEvent::ExplanationDelta(_) => {}
        }
    };

    let start_provider_time = std::time::Instant::now();
    let result = if config.stream {
        provider
            .generate_command_stream(
                &prompt,
                &os_info,
                &config.language,
                &session_context,
                custom_rules.as_deref(),
                &mut delta_handler,
            )
            .await
    } else {
        provider
            .generate_command(
                &prompt,
                &os_info,
                &config.language,
                &session_context,
                custom_rules.as_deref(),
            )
            .await
    };

    // Limpa a pré-visualização ou o spinner para renderizar o painel final sem duplicar texto
    if preview_started {
        for _ in 0..preview_lines {
            eprint!("\x1b[2K\x1b[1A");
        }
        eprint!("\r\x1b[2K");
        let _ = stderr().flush();
    } else if let Some(ref pb) = spinner {
        pb.finish_and_clear();
    }

    let suggestion = match result {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{} {}", "Erro ao comunicar com a IA:".red().bold(), e);

            let err_string = e.to_string();
            let is_auth_err = err_string.contains("401")
                || err_string.contains("Unauthorized")
                || err_string.contains("authentication_error")
                || err_string.contains("Authentication Fails")
                || err_string.contains("invalid api key")
                || err_string.contains("invalid_api_key");

            if is_auth_err {
                eprintln!("\n{}", "═".repeat(60).yellow());
                eprintln!("{}", " DIAGNÓSTICO DE AUTENTICAÇÃO / API KEY:".yellow().bold());
                eprintln!(" Falha de autenticação ao conectar com o provedor '{}' ({}).", config.provider.cyan(), config.api_url.cyan());
                eprintln!(" A chave de API configurada ({}) é inválida ou não foi autorizada.", config.masked_api_key().red());
                eprintln!(" Para configurar sua chave de API válida:");
                eprintln!("   {}", "tai config set api_key <sua-chave>".green().bold());
                eprintln!(" Ou defina a variável de ambiente:");
                eprintln!("   {}", "export TAI_API_KEY=<sua-chave>".cyan().bold());
                eprintln!(" Ou reconfigure tudo interativamente:");
                eprintln!("   {}", "tai config interactive".cyan().bold());
                eprintln!("{}\n", "═".repeat(60).yellow());
            } else {
                let is_model_err = (err_string.contains("model") && (err_string.contains("not found") || err_string.contains("404")))
                    || err_string.contains("supported API model names")
                    || (err_string.contains("400") && err_string.contains("model"));

                if is_model_err {
                    eprintln!("\n{}", "═".repeat(60).yellow());
                    eprintln!("{}", " DIAGNÓSTICO DE CONFIGURAÇÃO DE MODELO:".yellow().bold());
                    eprintln!(" O modelo '{}' é incompatível ou não foi encontrado no endpoint '{}'.", config.model.cyan(), config.api_url.cyan());
                    if config.model.contains(':') && config.provider != "ollama" {
                        eprintln!(" Parece ser um modelo local do Ollama configurado com provedor remoto.");
                        eprintln!(" Para ativar o Ollama local com este modelo:");
                        eprintln!("   {}", "tai config set provider ollama".green().bold());
                    } else if config.provider == "ollama" {
                        eprintln!(" Dica: Execute 'ollama list' para ver os modelos instalados.");
                        eprintln!(" Para corrigir, defina um modelo existente:");
                        eprintln!("   {}", "tai config set model <nome-do-modelo>".green().bold());
                    } else if let Some(spec) = ProviderSpec::find(&config.provider) {
                        eprintln!(" Para corrigir com o modelo padrão deste provedor:");
                        eprintln!("   {}", format!("tai config set model {}", spec.default_model).green().bold());
                    } else {
                        eprintln!(" Para corrigir com o modelo padrão da DeepSeek:");
                        eprintln!("   {}", "tai config set model deepseek-chat".green().bold());
                    }
                    eprintln!(" Ou reconfigure tudo interativamente:");
                    eprintln!("   {}", "tai config interactive".cyan().bold());
                    eprintln!("{}\n", "═".repeat(60).yellow());
                }
            }

            std::process::exit(1);
        }
    };
    let provider_duration = start_provider_time.elapsed();
    let latency_str = format_latency(provider_duration);

    let command = suggestion.command.trim();

    // Grava interação na sessão
    session.record_full(&prompt, command, Some(&latency_str), None, None);

    // Se a flag -p / --print estiver ativa, imprime apenas o comando limpo no stdout
    if cli.print {
        println!("{command}");
        return Ok(());
    }

    // Se a flag -c / --copy estiver ativa, copia e sai
    if cli.copy {
        if cli.quiet {
            println!("\n{} {}", "Comando gerado:".bold(), command.green());
        } else {
            println!(
                "\n{} {}  {}",
                "Comando gerado:".bold(),
                command.green(),
                format!("[⏱ {}]", latency_str).dimmed()
            );
        }
        let _ = CommandExecutor::copy_to_clipboard(command);
        return Ok(());
    }

    // Exibição visual com destaque
    println!("\n{}", "────────────────────────────────────────────────────────────".dimmed());
    if cli.quiet {
        println!("{} {}", "Comando Sugerido:".bold().cyan(), command.bold().green());
    } else {
        println!(
            "{} {}  {}",
            "Comando Sugerido:".bold().cyan(),
            command.bold().green(),
            format!("[⏱ {}]", latency_str).dimmed()
        );
    }
    
    // Se o comando tiver múltiplos passos encadeados (ex: &&), destaca os passos numerados
    if command.contains(" && ") {
        println!("{}", "\nPassos da execução:".bold().blue());
        for (idx, step) in command.split(" && ").enumerate() {
            let preview = CommandExecutor::format_step_preview(step);
            println!("  {}. {}", (idx + 1).to_string().cyan().bold(), preview.green());
        }
    }
    println!("{}", "────────────────────────────────────────────────────────────".dimmed());

    if !suggestion.explanation.is_empty() && !cli.quiet {
        println!("\n{}", "Explicação detalhada:".bold().yellow());
        for line in suggestion.explanation.lines() {
            println!("  {}", line.dimmed());
        }
        println!();
    }

    // Avaliação de segurança
    let safety = SafetyAnalyzer::assess(command);
    SafetyAnalyzer::print_warning(&safety);

    // Modo Inseguro com Trava Crítica
    if cli.yes {
        if safety.level == DangerLevel::Catastrophic {
            eprintln!("{}", "Execução abortada por segurança contra catástrofes.".red().bold());
            std::process::exit(1);
        }

        if suggestion.affects_parent_shell {
            if let Some(ref hook_file) = cli.shell_hook {
                std::fs::write(hook_file, command)?;
                session.record(&prompt, command);
                return Ok(());
            }
        }

        let exec_res = CommandExecutor::execute_with_capture(command).await?;
        session.record_full(&prompt, command, Some(&latency_str), Some(format!("{}\n{}", exec_res.stdout, exec_res.stderr)), Some(exec_res.exit_code));
        return Ok(());
    }

    // Menu Interativo
    let options = vec![
        "Executar agora",
        "Copiar para área de transferência",
        "Cancelar",
    ];

    let choice = Select::new("O que deseja fazer?", options).prompt()?;

    match choice {
        "Executar agora" => {
            if suggestion.affects_parent_shell {
                if let Some(ref hook_file) = cli.shell_hook {
                    std::fs::write(hook_file, command)?;
                    session.record_full(&prompt, command, Some(&latency_str), None, None);
                    println!("{}", "✓ Comando enviado para execução na sessão atual do seu terminal!".green().bold());
                    return Ok(());
                } else {
                    println!("{}", "💡 Dica: Este comando altera o ambiente do shell (como cd ou export).".yellow());
                    println!("   Para que comandos como 'cd' alterem a pasta da sua sessão ativa de verdade,");
                    println!("   certifique-se de que o hook do tai esteja ativo no seu shell (~/.bashrc).");
                }
            }

            let exec_res = CommandExecutor::execute_with_capture(command).await?;
            session.record_full(&prompt, command, Some(&latency_str), Some(format!("{}\n{}", exec_res.stdout, exec_res.stderr)), Some(exec_res.exit_code));

            if exec_res.exit_code != 0 && !cli.quiet && !cli.yes {
                println!("{}", "\n💡 O comando terminou com erro.".yellow().bold());
                let autoheal_choices = vec!["Sim, diagnosticar erro e sugerir correção", "Não, sair"];
                if let Ok("Sim, diagnosticar erro e sugerir correção") = Select::new("Deseja que a IA analise a falha e tente corrigir?", autoheal_choices).prompt() {
                    let err_log = if !exec_res.stderr.is_empty() {
                        &exec_res.stderr
                    } else if !exec_res.stdout.is_empty() {
                        &exec_res.stdout
                    } else {
                        "Comando finalizou com erro mas sem saida."
                    };

                    let heal_prompt = format!("O comando anterior `{command}` falhou com código {}.\nSaída / Erro observado:\n```\n{}\n```\nPor favor analise o erro e forneça o comando de correção.", exec_res.exit_code, err_log.trim());
                    println!("{}", "🔍 Consultando IA para correção do erro...".cyan());

                    let heal_start = std::time::Instant::now();
                    if let Ok(fix_suggestion) = provider.generate_command(&heal_prompt, &os_info, &config.language, &session_context, custom_rules.as_deref()).await {
                        let heal_duration = heal_start.elapsed();
                        let heal_latency_str = format_latency(heal_duration);
                        let fix_cmd = fix_suggestion.command.trim();
                        println!("\n{}", "────────────────────────────────────────────────────────────".dimmed());
                        println!(
                            "{} {}  {}",
                            "Comando de Correção Sugerido:".bold().yellow(),
                            fix_cmd.bold().green(),
                            format!("[⏱ {}]", heal_latency_str).dimmed()
                        );
                        if !fix_suggestion.explanation.is_empty() {
                            println!("\n{}", "Diagnóstico da Correção:".bold().cyan());
                            for line in fix_suggestion.explanation.lines() {
                                println!("  {}", line.dimmed());
                            }
                        }
                        println!("{}", "────────────────────────────────────────────────────────────".dimmed());

                        let fix_choices = vec!["Executar comando de correção agora", "Copiar para clipboard", "Cancelar"];
                        if let Ok(fix_choice) = Select::new("O que deseja fazer?", fix_choices).prompt() {
                            match fix_choice {
                                "Executar comando de correção agora" => {
                                    if fix_suggestion.affects_parent_shell {
                                        if let Some(ref hook_file) = cli.shell_hook {
                                            let _ = std::fs::write(hook_file, fix_cmd);
                                            session.record_full(&heal_prompt, fix_cmd, Some(&heal_latency_str), None, None);
                                            println!("{}", "✓ Comando de correção enviado para execução na sessão atual!".green().bold());
                                            return Ok(());
                                        }
                                    }
                                    let fix_exec_res = CommandExecutor::execute_with_capture(fix_cmd).await?;
                                    session.record_full(&heal_prompt, fix_cmd, Some(&heal_latency_str), Some(format!("{}\n{}", fix_exec_res.stdout, fix_exec_res.stderr)), Some(fix_exec_res.exit_code));
                                    std::process::exit(fix_exec_res.exit_code);
                                }
                                "Copiar para clipboard" => {
                                    let _ = CommandExecutor::copy_to_clipboard(fix_cmd);
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            std::process::exit(exec_res.exit_code);
        }
        "Copiar para área de transferência" => {
            let _ = CommandExecutor::copy_to_clipboard(command);
        }
        _ => {
            println!("{}", "Operação cancelada.".dimmed());
        }
    }

    Ok(())
}

async fn run_interactive_config(config: &mut Config) -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", "════════════════════════════════════════════════════════════".cyan());
    println!("{}", " CONFIGURAÇÃO INTERATIVA DO TERMINAL AI (TAI)".bold().cyan());
    println!("{}\n", "════════════════════════════════════════════════════════════".cyan());

    let provider_options = vec![
        "DeepSeek (api.deepseek.com)",
        "Google Gemini (generativelanguage.googleapis.com)",
        "OpenAI (api.openai.com)",
        "Anthropic Claude (api.anthropic.com)",
        "Mistral AI / Codestral (api.mistral.ai)",
        "Alibaba Qwen / DashScope (dashscope-intl.aliyuncs.com)",
        "Zhipu AI / GLM (open.bigmodel.cn)",
        "Moonshot AI / Kimi (api.moonshot.cn)",
        "01.AI / Yi (api.01.ai)",
        "Groq (api.groq.com)",
        "OpenRouter (openrouter.ai)",
        "Ollama - Modelo local (localhost:11434)",
        "Personalizado / Inserir outro provedor",
    ];

    let default_idx = match config.provider.as_str() {
        "deepseek" => 0,
        "gemini" => 1,
        "openai" => 2,
        "anthropic" => 3,
        "mistral" => 4,
        "qwen" => 5,
        "zhipu" => 6,
        "moonshot" => 7,
        "yi" => 8,
        "groq" => 9,
        "openrouter" => 10,
        "ollama" => 11,
        _ => 12,
    };

    let selected_provider = Select::new("Selecione o provedor de IA:", provider_options)
        .with_starting_cursor(default_idx)
        .prompt()?;

    if selected_provider.starts_with("Ollama") {
        config.provider = "ollama".to_string();
        let default_ollama_url = if !config.ollama_url.is_empty() {
            config.ollama_url.as_str()
        } else {
            "http://localhost:11434/v1"
        };
        let ollama_url_input = Text::new("Informe a URL do daemon Ollama:")
            .with_default(default_ollama_url)
            .prompt()?;
        let mut trimmed_url = ollama_url_input.trim().trim_end_matches('/').to_string();
        if trimmed_url == "http://localhost:11434" || trimmed_url == "http://127.0.0.1:11434" {
            trimmed_url = format!("{trimmed_url}/v1");
        }
        config.ollama_url = trimmed_url.clone();
        config.api_url = trimmed_url;

        // Tenta detectar modelos instalados no daemon local do Ollama
        let mut detected_models = Vec::new();
        let ping_url = if config.api_url.ends_with("/v1") {
            format!("{}/api/tags", config.api_url.trim_end_matches("/v1"))
        } else {
            format!("{}/api/tags", config.api_url)
        };
        if let Ok(client) = reqwest::Client::builder().timeout(Duration::from_millis(1500)).build() {
            if let Ok(resp) = client.get(&ping_url).send().await {
                if let Ok(tags_json) = resp.json::<serde_json::Value>().await {
                    if let Some(arr) = tags_json.get("models").and_then(|v| v.as_array()) {
                        for item in arr {
                            if let Some(name) = item.get("name").and_then(|v| v.as_str()) {
                                detected_models.push(name.to_string());
                            }
                        }
                    }
                }
            }
        }

        if !detected_models.is_empty() {
            println!("  ✓ {} modelo(s) encontrado(s) no Ollama local.", detected_models.len().to_string().green());
            let mut choices = detected_models;
            choices.push("Outro (digitar manualmente)".to_string());

            let selected = Select::new("Escolha o modelo local instalado no Ollama:", choices).prompt()?;
            if selected == "Outro (digitar manualmente)" {
                let default_val = if !config.model.is_empty() && config.model != "deepseek-chat" {
                    config.model.as_str()
                } else {
                    "qwen2.5-coder:1.5b"
                };
                config.model = Text::new("Informe o nome do modelo do Ollama:")
                    .with_default(default_val)
                    .prompt()?;
            } else {
                config.model = selected;
            }
        } else {
            let default_val = if !config.model.is_empty() && config.model != "deepseek-chat" {
                config.model.as_str()
            } else {
                "qwen2.5-coder:1.5b"
            };
            config.model = Text::new("Informe o nome do modelo do Ollama:")
                .with_default(default_val)
                .with_help_message("Ex: qwen2.5-coder:1.5b, yi-coder:1.5b, llama3:8b")
                .prompt()?;
        }

        // Mantém a api_key salva se houver uma chave remota, ou usa dummy se estiver vazia
        if config.api_key.is_empty() {
            config.api_key = "ollama".to_string();
        }
    } else if selected_provider.starts_with("Personalizado") {
        println!("\n{}", "--- Configuração de Provedor Personalizado ---".bold().yellow());
        let default_prov = if config.provider != "ollama" && !config.provider.is_empty() {
            config.provider.as_str()
        } else {
            "custom"
        };
        config.provider = Text::new("Informe o nome do provedor:")
            .with_default(default_prov)
            .prompt()?.trim().to_lowercase();

        let default_url = if !config.api_url.is_empty() && !config.api_url.contains("localhost") {
            config.api_url.as_str()
        } else {
            "https://meu-endpoint.com/v1"
        };
        let url_input = Text::new("Informe a URL da API (endpoint compatível com OpenAI):")
            .with_default(default_url)
            .prompt()?;
        config.api_url = url_input.trim().trim_end_matches('/').to_string();

        let default_model = if !config.model.is_empty() {
            config.model.as_str()
        } else {
            "meu-modelo"
        };
        config.model = Text::new("Informe o nome do modelo:")
            .with_default(default_model)
            .prompt()?;

        let default_key = if config.api_key != "ollama" && !config.api_key.is_empty() {
            config.api_key.as_str()
        } else {
            ""
        };
        let key_input = Text::new("Informe a chave de API (API Key, deixe vazio se não houver):")
            .with_default(default_key)
            .prompt()?;
        config.api_key = key_input.trim().to_string();
    } else {
        // Provedor conhecido da lista fixa
        let provider_id = if selected_provider.starts_with("DeepSeek") {
            "deepseek"
        } else if selected_provider.starts_with("Google Gemini") {
            "gemini"
        } else if selected_provider.starts_with("OpenAI") {
            "openai"
        } else if selected_provider.starts_with("Anthropic") {
            "anthropic"
        } else if selected_provider.starts_with("Mistral") {
            "mistral"
        } else if selected_provider.starts_with("Alibaba Qwen") {
            "qwen"
        } else if selected_provider.starts_with("Zhipu") {
            "zhipu"
        } else if selected_provider.starts_with("Moonshot") {
            "moonshot"
        } else if selected_provider.starts_with("01.AI") {
            "yi"
        } else if selected_provider.starts_with("Groq") {
            "groq"
        } else {
            "openrouter"
        };

        let spec = ProviderSpec::find(provider_id).expect("Provedor conhecido deve existir no catálogo");
        config.provider = spec.id.to_string();

        println!("\n{}", format!("--- Configuração {} ---", spec.name).bold().yellow());

        let url_input = Text::new("Informe a URL da API:")
            .with_default(spec.default_url)
            .prompt()?;
        config.api_url = url_input.trim().trim_end_matches('/').to_string();

        let mut model_choices: Vec<String> = spec.models.iter().map(|&s| s.to_string()).collect();
        model_choices.push("Outro (digitar manualmente)".to_string());

        let selected_model = Select::new("Escolha o modelo:", model_choices).prompt()?;
        if selected_model == "Outro (digitar manualmente)" {
            config.model = Text::new("Informe o nome do modelo:")
                .with_default(spec.default_model)
                .prompt()?;
        } else {
            config.model = selected_model;
        }

        let default_key = if config.api_key != "ollama" && !config.api_key.is_empty() {
            config.api_key.as_str()
        } else {
            ""
        };

        let key_input = Text::new("Informe a chave de API (API Key):")
            .with_default(default_key)
            .prompt()?;

        config.api_key = key_input.trim().to_string();
    }

    config.save()?;
    println!("\n{}", "✓ Configurações salvas em config.toml com sucesso!".green().bold());
    println!("  Provedor:     {}", config.provider.cyan());
    println!("  Modelo:       {}", config.model.cyan());
    println!("  API URL:      {}", config.api_url.cyan());
    println!("  API Key:      {}", config.masked_api_key().cyan());
    println!("  Ollama URL:   {}", config.ollama_url.cyan());
    println!("  Idioma:       {}", config.language.cyan());
    println!("  Streaming:    {}", if config.stream { "ativado".cyan() } else { "desativado".yellow() });
    Ok(())
}

