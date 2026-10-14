use colored::Colorize;
use serde::Deserialize;
use std::process::Command;

#[derive(Deserialize, Debug)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
}

pub struct AutoUpdater;

impl AutoUpdater {
    pub async fn check_and_update() -> Result<(), Box<dyn std::error::Error>> {
        let current_version = env!("CARGO_PKG_VERSION");
        println!(
            "{}",
            format!("Versão instalada: v{current_version}").cyan().bold()
        );
        println!("{}", "Verificando atualizações no GitHub...".dimmed());

        let client = reqwest::Client::builder()
            .user_agent("terminal-ai-updater")
            .build()?;

        let repo_url = "https://api.github.com/repos/SrClauss/terminal_ai/releases/latest";
        let resp = client.get(repo_url).send().await;

        match resp {
            Ok(response) if response.status().is_success() => {
                let release: GitHubRelease = response.json().await?;
                let latest_tag = release.tag_name.trim_start_matches('v');

                if latest_tag != current_version {
                    println!(
                        "{}",
                        format!("Nova versão disponível: v{latest_tag}!").green().bold()
                    );
                    println!("Release URL: {}", release.html_url.underline());
                    println!("\n{}", "Para atualizar via cargo:".yellow());
                    println!("  cargo install --git https://github.com/SrClauss/terminal_ai --force");
                } else {
                    println!("{}", "Você já está utilizando a versão mais recente!".green());
                }
            }
            _ => {
                // Fallback: se não houver releases no GitHub, tenta verificar via git pull se estiver no diretório clonado
                println!("{}", "Nenhuma release oficial publicada ainda. Verificando via repositório Git...".yellow());
                if std::path::Path::new(".git").exists() {
                    let mut child = Command::new("git")
                        .args(["pull"])
                        .spawn()?;
                    let status = child.wait()?;
                    if status.success() {
                        println!("{}", "Repositório sincronizado com sucesso!".green());
                    }
                } else {
                    println!("Repositório: https://github.com/SrClauss/terminal_ai");
                }
            }
        }

        Ok(())
    }

    pub fn rebuild_local() -> Result<(), Box<dyn std::error::Error>> {
        let repo_candidates = [
            "/home/claus/src/terminal_ai",
            "/media/claus/Arquivos/src/terminal_ai",
            ".",
        ];

        let mut found_dir = None;
        for path in repo_candidates {
            let p = std::path::Path::new(path);
            if p.join("Cargo.toml").exists() && p.join("src/main.rs").exists() {
                found_dir = Some(p);
                break;
            }
        }

        let repo_dir = match found_dir {
            Some(d) => d,
            None => {
                eprintln!("{}", "Código fonte de terminal_ai não encontrado nos caminhos padrão.".red());
                return Ok(());
            }
        };

        println!(
            "{}",
            format!("Recompilando Terminal AI a partir de: {}", repo_dir.display()).cyan().bold()
        );

        // 1. Executa testes
        println!("{}", "Executando testes (cargo test)...".dimmed());
        let test_status = Command::new("cargo")
            .current_dir(repo_dir)
            .args(["test"])
            .status()?;

        if !test_status.success() {
            eprintln!("{}", "Falha nos testes! Abortando recompilação para segurança.".red().bold());
            return Ok(());
        }

        // 2. Executa instalação
        let install_script = repo_dir.join("install.sh");
        if install_script.exists() {
            println!("{}", "Instalando nova versão via install.sh...".dimmed());
            let install_status = Command::new("bash")
                .current_dir(repo_dir)
                .arg("install.sh")
                .status()?;

            if install_status.success() {
                println!("{}", "Terminal AI recompilado e reinstalado com sucesso!".green().bold());
            } else {
                eprintln!("{}", "Falha ao executar install.sh.".red());
            }
        } else {
            // Fallback para cargo build --release
            println!("{}", "Compilando em modo release...".dimmed());
            let build_status = Command::new("cargo")
                .current_dir(repo_dir)
                .args(["build", "--release"])
                .status()?;

            if build_status.success() {
                println!("{}", "Compilação concluída com sucesso!".green().bold());
            }
        }

        Ok(())
    }
}
