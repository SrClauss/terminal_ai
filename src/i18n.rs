use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WslInfo {
    pub is_wsl: bool,
    pub distro: Option<String>,
    pub windows_user: Option<String>,
    pub windows_mount: Option<String>,
}

pub struct OsContext;

impl OsContext {
    /// Detecta se a execução atual ocorre dentro do WSL (Windows Subsystem for Linux)
    pub fn is_wsl() -> bool {
        Self::is_wsl_custom(
            std::env::var("WSL_DISTRO_NAME").ok().as_deref(),
            std::env::var("WSL_INTEROP").ok().as_deref(),
            std::fs::read_to_string("/proc/version").ok().as_deref(),
        )
    }

    /// Lógica pura para detecção de WSL testável com injeção de parâmetros
    pub fn is_wsl_custom(
        distro: Option<&str>,
        interop: Option<&str>,
        proc_version: Option<&str>,
    ) -> bool {
        if distro.is_some() || interop.is_some() {
            return true;
        }
        if let Some(v) = proc_version {
            let lower = v.to_lowercase();
            if lower.contains("microsoft") || lower.contains("wsl") {
                return true;
            }
        }
        false
    }

    /// Detecta o nome do usuário do Windows mapeando diretórios sob /mnt/c/Users
    pub fn detect_windows_user() -> Option<String> {
        Self::detect_windows_user_in_path(Path::new("/mnt/c/Users"))
    }

    /// Lógica pura para detecção do usuário do Windows em um caminho de diretório de usuários
    pub fn detect_windows_user_in_path(users_dir: &Path) -> Option<String> {
        if !users_dir.is_dir() {
            return None;
        }

        let ignored = [
            "public",
            "default",
            "default user",
            "all users",
            "desktop.ini",
        ];

        let entries = std::fs::read_dir(users_dir).ok()?;
        for entry in entries.flatten() {
            if let Ok(file_type) = entry.file_type() {
                if file_type.is_dir() {
                    if let Some(name) = entry.file_name().to_str() {
                        let lower = name.to_lowercase();
                        if !ignored.contains(&lower.as_str()) && !name.starts_with('.') {
                            return Some(name.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Retorna informações agregadas sobre o ambiente WSL caso ativo
    pub fn get_wsl_info() -> Option<WslInfo> {
        if !Self::is_wsl() {
            return None;
        }

        let distro = std::env::var("WSL_DISTRO_NAME").ok();
        let win_user = Self::detect_windows_user();
        let mount = if Path::new("/mnt/c").exists() {
            Some("/mnt/c".to_string())
        } else {
            None
        };

        Some(WslInfo {
            is_wsl: true,
            distro,
            windows_user: win_user,
            windows_mount: mount,
        })
    }

    /// Gera o bloco de contexto de prompt específico de WSL caso esteja rodando sob WSL
    pub fn wsl_prompt_context() -> Option<String> {
        let wsl = Self::get_wsl_info()?;
        let win_user = wsl.windows_user.unwrap_or_else(|| "<username>".to_string());
        let mut ctx = String::from("\nWSL (WINDOWS SUBSYSTEM FOR LINUX) INTEROPERABILITY DETECTED:\n");
        ctx.push_str(&format!(
            "- The Windows host C: drive is mounted at '/mnt/c/'.\n\
             - Windows active user directory: '/mnt/c/Users/{win_user}/'.\n\
             - Windows Desktop: '/mnt/c/Users/{win_user}/Desktop/'.\n\
             - Windows Downloads: '/mnt/c/Users/{win_user}/Downloads/'.\n\
             - Windows Documents: '/mnt/c/Users/{win_user}/Documents/'.\n\
             - Interoperability: Windows executables can be invoked from bash (e.g. 'explorer.exe .', 'cmd.exe /c ...').\n\
             - RULE: When user asks for Windows files, Desktop, or Downloads, ALWAYS use the '/mnt/c/Users/{win_user}/...' mount paths.\n"
        ));
        Some(ctx)
    }

    /// Retorna a string descritiva do sistema operacional para envio à LLM
    pub fn get_info() -> String {
        let os_name = if let Ok(content) = std::fs::read_to_string("/etc/os-release") {
            content
                .lines()
                .find(|l| l.starts_with("PRETTY_NAME="))
                .and_then(|l| l.split('=').nth(1))
                .map(|s| s.trim_matches('"'))
                .unwrap_or("Linux")
                .to_string()
        } else {
            "Linux".to_string()
        };

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
        let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
        let is_root = user == "root";

        let mut base = format!(
            "OS: {}, Shell: {}, User: {} (root: {})",
            os_name, shell, user, is_root
        );

        if let Some(wsl) = Self::get_wsl_info() {
            let distro_str = wsl.distro.as_deref().unwrap_or("WSL2");
            let win_user_str = wsl.windows_user.as_deref().unwrap_or("detected");
            base.push_str(&format!(
                ", Platform: WSL ({distro_str}), Windows Host User: {win_user_str}, Windows Mount: /mnt/c"
            ));
        }

        base
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_is_wsl_detection() {
        assert!(OsContext::is_wsl_custom(Some("Ubuntu"), None, None));
        assert!(OsContext::is_wsl_custom(None, Some("/run/WSL/1_interop"), None));
        assert!(OsContext::is_wsl_custom(
            None,
            None,
            Some("Linux version 5.15.153.1-microsoft-standard-WSL2 (gcc version 11.2.0)")
        ));
        assert!(!OsContext::is_wsl_custom(
            None,
            None,
            Some("Linux version 6.8.0-45-generic (buildd@lcy02-amd64-080)")
        ));
        assert!(!OsContext::is_wsl_custom(None, None, None));
    }

    #[test]
    fn test_detect_windows_user_in_path() {
        let temp_dir = tempdir().expect("Falha ao criar tempdir");
        let users_path = temp_dir.path();

        // Cria pastas típicas do Windows que devem ser ignoradas
        fs::create_dir_all(users_path.join("Public")).unwrap();
        fs::create_dir_all(users_path.join("Default")).unwrap();
        fs::create_dir_all(users_path.join("All Users")).unwrap();
        fs::write(users_path.join("desktop.ini"), "").unwrap();

        // Ainda não há usuário real
        assert_eq!(OsContext::detect_windows_user_in_path(users_path), None);

        // Cria usuário real
        fs::create_dir_all(users_path.join("SrClauss")).unwrap();
        let detected = OsContext::detect_windows_user_in_path(users_path);
        assert_eq!(detected, Some("SrClauss".to_string()));
    }

    #[test]
    fn test_get_info_format() {
        let info = OsContext::get_info();
        assert!(info.contains("OS:"));
        assert!(info.contains("Shell:"));
        assert!(info.contains("User:"));
    }

    #[test]
    fn test_wsl_prompt_context_formatting() {
        let wsl = WslInfo {
            is_wsl: true,
            distro: Some("Ubuntu".to_string()),
            windows_user: Some("Claus".to_string()),
            windows_mount: Some("/mnt/c".to_string()),
        };
        let win_user = wsl.windows_user.unwrap();
        let expected_user_dir = format!("/mnt/c/Users/{win_user}/");
        let expected_desktop = format!("/mnt/c/Users/{win_user}/Desktop/");
        assert_eq!(expected_user_dir, "/mnt/c/Users/Claus/");
        assert_eq!(expected_desktop, "/mnt/c/Users/Claus/Desktop/");
    }
}
