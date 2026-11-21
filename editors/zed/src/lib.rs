use zed_extension_api::{self as zed, Command, LanguageServerId, Result, Worktree};

struct HartExtension;

impl zed::Extension for HartExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &Worktree,
    ) -> Result<Command> {
        if language_server_id.as_ref() == "hart" {
            let binary_path = worktree
                .which("hart")
                .ok_or_else(|| "hart binary not found on PATH. Run `cargo install --path .`".to_string())?;

            Ok(Command {
                command: binary_path,
                args: vec!["lsp".to_string()],
                env: Default::default(),
            })
        } else {
            Err(format!("Unknown language server ID: {language_server_id}"))
        }
    }
}

zed::register_extension!(HartExtension);

