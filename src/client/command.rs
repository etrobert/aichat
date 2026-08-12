use super::*;

use anyhow::{bail, Context, Result};
use reqwest::Client as ReqwestClient;
use serde::Deserialize;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Deserialize)]
pub struct CommandConfig {
    pub name: Option<String>,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub models: Vec<ModelData>,
    pub patch: Option<RequestPatch>,
    pub extra: Option<ExtraConfig>,
}

impl CommandClient {
    config_get_fn!(command, get_command);

    pub const PROMPTS: [PromptAction<'static>; 1] = [("command", "Command", None)];

    fn build_argv(&self, data: &ChatCompletionsData) -> Result<(String, Vec<String>, bool)> {
        let command = self.get_command()?;
        let system = render_role(&data.messages, |role| role.is_system());
        let prompt = render_prompt(&data.messages);
        let mut piped = true;
        let args = self
            .config
            .args
            .iter()
            .map(|arg| {
                if arg.contains("{prompt}") {
                    piped = false;
                }
                arg.replace("{model}", self.model.real_name())
                    .replace("{system}", &system)
                    .replace("{prompt}", &prompt)
            })
            .collect();
        Ok((command, args, piped))
    }

    async fn call(&self, data: ChatCompletionsData) -> Result<String> {
        let (command, args, piped) = self.build_argv(&data)?;
        let prompt = render_prompt(&data.messages);
        debug!("Command Request: {command} {args:?}");
        // The command is blocking, so it runs off the runtime: the streaming
        // path selects on this future against the abort signal.
        let output = tokio::task::spawn_blocking(move || {
            let mut child = Command::new(&command)
                .args(&args)
                .stdin(if piped {
                    Stdio::piped()
                } else {
                    Stdio::null()
                })
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("Failed to run '{command}'"))?;
            if piped {
                if let Some(stdin) = child.stdin.as_mut() {
                    use std::io::Write;
                    stdin.write_all(prompt.as_bytes())?;
                }
                drop(child.stdin.take());
            }
            child.wait_with_output().context("Failed to read output")
        })
        .await??;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stderr = stderr.trim();
            match stderr.is_empty() {
                true => bail!("Command exited with {}", output.status),
                false => bail!("{stderr}"),
            }
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

#[async_trait::async_trait]
impl Client for CommandClient {
    client_common_fns!();

    async fn chat_completions_inner(
        &self,
        _client: &ReqwestClient,
        data: ChatCompletionsData,
    ) -> Result<ChatCompletionsOutput> {
        let text = self.call(data).await?;
        Ok(ChatCompletionsOutput::new(&text))
    }

    async fn chat_completions_streaming_inner(
        &self,
        _client: &ReqwestClient,
        handler: &mut SseHandler,
        data: ChatCompletionsData,
    ) -> Result<()> {
        // A command has no stream to follow, so the whole reply arrives at once.
        let text = self.call(data).await?;
        handler.text(&text)?;
        Ok(())
    }
}

fn render_role(messages: &[Message], filter: impl Fn(&MessageRole) -> bool) -> String {
    messages
        .iter()
        .filter(|message| filter(&message.role))
        .map(|message| message.content.to_text())
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// Everything but the system messages, labelled by role once there is a history
/// to keep straight.
fn render_prompt(messages: &[Message]) -> String {
    let turns: Vec<&Message> = messages
        .iter()
        .filter(|message| !message.role.is_system())
        .collect();
    if turns.len() == 1 {
        return turns[0].content.to_text().trim().to_string();
    }
    turns
        .iter()
        .map(|message| {
            let role = match message.role {
                MessageRole::Assistant => "assistant",
                MessageRole::Tool => "tool",
                _ => "user",
            };
            format!("{role}: {}", message.content.to_text())
        })
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}
