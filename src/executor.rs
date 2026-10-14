use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

pub struct ExecutionResult {
    pub exit_code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub struct CommandExecutor;

impl CommandExecutor {
    /// Formata um comando ou passo para exibição elegante no CLI, truncando scripts multilinhas
    pub fn format_step_preview(step: &str) -> String {
        let trimmed = step.trim();
        let mut lines = trimmed.lines().map(|l| l.trim_end());
        if let Some(first_line) = lines.next() {
            let other_lines_count = trimmed.lines().count().saturating_sub(1);
            if other_lines_count > 0 {
                format!("{} {}", first_line.trim(), format!("[+{} linha(s) de script]", other_lines_count).dimmed().italic())
            } else {
                first_line.trim().to_string()
            }
        } else {
            String::new()
        }
    }

    /// Executa um comando com streaming em tempo real no terminal e feedback ativo durante comandos longos
    pub async fn execute_with_capture(command_str: &str) -> Result<ExecutionResult, std::io::Error> {
        println!("{}", "
--- Executando comando ---".dimmed());

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".to_string());
        let mut child = Command::new(shell)
            .arg("-c")
            .arg(command_str)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take().expect("Failed to open child stdout");
        let stderr = child.stderr.take().expect("Failed to open child stderr");

        let mut stdout_reader = BufReader::new(stdout).lines();
        let mut stderr_reader = BufReader::new(stderr).lines();

        let mut collected_stdout = Vec::new();
        let mut collected_stderr = Vec::new();

        if command_str.contains(" && ") {
            let steps: Vec<&str> = command_str.split(" && ").collect();
            println!("{}", format!("▶ Executando {} passos encadeados:", steps.len()).bold().blue());
            for (idx, step) in steps.iter().enumerate() {
                let preview = Self::format_step_preview(step);
                println!("  {}. {}", (idx + 1).to_string().cyan().bold(), preview.green());
            }
        }

        // Indicador de progresso dinâmico para quando o comando estiver sem emitir linhas
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
                .template("{spinner:.cyan} {msg}")
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        spinner.enable_steady_tick(Duration::from_millis(100));

        let start_time = Instant::now();
        let mut interval = tokio::time::interval(Duration::from_millis(100));
        let mut has_emitted_any = false;

        loop {
            tokio::select! {
                _ = interval.tick() => {
                    let elapsed = start_time.elapsed().as_secs();
                    let mins = elapsed / 60;
                    let secs = elapsed % 60;
                    let time_str = format!("{:02}:{:02}", mins, secs);

                    if !has_emitted_any {
                        if elapsed >= 3 {
                            spinner.set_message(format!(
                                "{} [{}] {}",
                                "Executando tarefa complexa...".cyan().bold(),
                                time_str.yellow().bold(),
                                "(em andamento, aguardando saída...)".dimmed()
                            ));
                        } else {
                            spinner.set_message(format!(
                                "{} [{}] {}",
                                "Processando comando...".cyan(),
                                time_str.yellow().bold(),
                                "(pressione Ctrl+C para cancelar)".dimmed()
                            ));
                        }
                    } else {
                        let lines_count = collected_stdout.len() + collected_stderr.len();
                        spinner.set_message(format!(
                            "{} [{}] {}",
                            "Executando tarefa...".cyan(),
                            time_str.yellow().bold(),
                            format!("({lines_count} linha(s) processada(s))").dimmed()
                        ));
                    }
                }
                line_res = stdout_reader.next_line() => {
                    match line_res {
                        Ok(Some(line)) => {
                            has_emitted_any = true;
                            spinner.suspend(|| {
                                println!("{line}");
                            });
                            collected_stdout.push(line);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("Erro ao ler stdout: {e}");
                        }
                    }
                }
                err_res = stderr_reader.next_line() => {
                    match err_res {
                        Ok(Some(line)) => {
                            has_emitted_any = true;
                            spinner.suspend(|| {
                                eprintln!("{}", line.red());
                            });
                            collected_stderr.push(line);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            eprintln!("Erro ao ler stderr: {e}");
                        }
                    }
                }
                status_res = child.wait() => {
                    spinner.finish_and_clear();
                    // Garante que a linha do terminal onde o spinner estava seja limpa completamente
                    eprint!("\r\x1b[2K");
                    let _ = std::io::Write::flush(&mut std::io::stderr());

                    let status = status_res?;
                    let exit_code = status.code().unwrap_or(0);
                    let total_secs = start_time.elapsed().as_secs_f64();

                    // Esvazia buffers restantes se houver
                    while let Ok(Some(line)) = stdout_reader.next_line().await {
                        println!("{line}");
                        collected_stdout.push(line);
                    }
                    while let Ok(Some(line)) = stderr_reader.next_line().await {
                        eprintln!("{}", line.red());
                        collected_stderr.push(line);
                    }

                    println!("{}", "--------------------------".dimmed());

                    let full_stdout = collected_stdout.join("\n");
                    let full_stderr = collected_stderr.join("\n");

                    if exit_code == 0 {
                        if full_stdout.trim().is_empty() {
                            println!("{}", format!("ℹ️  O comando rodou com sucesso em {total_secs:.2}s, mas nenhuma saída foi emitida.").yellow());
                        } else {
                            let lines_count = collected_stdout.len();
                            println!("{}", format!("✓ Concluído com sucesso ({lines_count} linha(s) de saída em {total_secs:.2}s).").green().bold());
                        }
                    } else {
                        if full_stdout.trim().is_empty() && full_stderr.trim().is_empty() {
                            println!("{}", format!("ℹ️  O comando terminou com código {exit_code} em {total_secs:.2}s sem retornar correspondências.").yellow());
                        } else {
                            println!("{}", format!("❌ O comando terminou com código de erro {exit_code} (em {total_secs:.2}s).").red().bold());
                        }
                    }

                    return Ok(ExecutionResult {
                        exit_code,
                        stdout: full_stdout,
                        stderr: full_stderr,
                    });
                }
            }
        }
    }

    pub fn copy_to_clipboard(command_str: &str) -> Result<(), String> {
        match arboard::Clipboard::new() {
            Ok(mut clipboard) => {
                clipboard
                    .set_text(command_str.to_string())
                    .map_err(|e| format!("Erro ao copiar via clipboard: {e}"))?;
                println!("{}", "Comando copiado para a área de transferência!".green().bold());
                Ok(())
            }
            Err(_) => {
                // Fallback para ferramentas de linha de comando no Linux (wl-copy ou xclip)
                if let Ok(mut child) = std::process::Command::new("wl-copy").stdin(Stdio::piped()).spawn() {
                    use std::io::Write;
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(command_str.as_bytes());
                    }
                    let _ = child.wait();
                    println!("{}", "Comando copiado via wl-copy!".green().bold());
                    return Ok(());
                }

                if let Ok(mut child) = std::process::Command::new("xclip").args(["-selection", "clipboard"]).stdin(Stdio::piped()).spawn() {
                    use std::io::Write;
                    if let Some(mut stdin) = child.stdin.take() {
                        let _ = stdin.write_all(command_str.as_bytes());
                    }
                    let _ = child.wait();
                    println!("{}", "Comando copiado via xclip!".green().bold());
                    return Ok(());
                }

                Err("Nenhum utilitário de clipboard disponível (arboard, wl-copy ou xclip).".to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_step_preview_single_line() {
        let single = "cargo check --all-targets";
        assert_eq!(CommandExecutor::format_step_preview(single), "cargo check --all-targets");
    }

    #[test]
    fn test_format_step_preview_multiline() {
        let script = r#"python3 -c "
import sys
for i in range(10):
    print(i)
""#;
        let preview = CommandExecutor::format_step_preview(script);
        assert!(preview.contains("python3 -c \""));
        assert!(preview.contains("[+4 linha(s) de script]"));
    }

    #[test]
    fn test_format_step_preview_empty() {
        assert_eq!(CommandExecutor::format_step_preview(""), "");
        assert_eq!(CommandExecutor::format_step_preview("   \n  \n  "), "");
    }

    #[test]
    fn test_format_step_preview_pipe_and_heredoc() {
        let heredoc_cmd = "cat << 'EOF' > script.py\nimport os\nprint(os.getcwd())\nEOF";
        let preview = CommandExecutor::format_step_preview(heredoc_cmd);
        assert!(preview.contains("cat << 'EOF' > script.py"));
        assert!(preview.contains("[+3 linha(s) de script]"));

        let piped = "ps aux | grep node | awk '{print $2}'";
        assert_eq!(CommandExecutor::format_step_preview(piped), piped);
    }

    #[test]
    fn test_format_step_preview_bash_loop() {
        let bash_loop = "for f in *.txt; do\n  echo \"Processing $f\"\n  wc -l \"$f\"\ndone";
        let preview = CommandExecutor::format_step_preview(bash_loop);
        assert!(preview.contains("for f in *.txt; do"));
        assert!(preview.contains("[+3 linha(s) de script]"));
    }
}
