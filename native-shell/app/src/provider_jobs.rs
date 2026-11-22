use std::time::Duration;

use govue_shell_runtime::{Core, CoreError};
use nsh_core::{
    auth_session::{AuthSessionOutcome, AuthSessionRequest},
    element::{close_auth_session, open_auth_session},
    prelude::*,
};
use serde_json::{Value, json};
use url::Url;

#[derive(Clone, Copy, PartialEq)]
pub enum Job {
    Login,
    Test,
    Models,
    Remove,
    Disconnect,
}

impl Job {
    fn kind(self) -> &'static str {
        match self {
            Self::Login => "login",
            Self::Test => "chat",
            Self::Models => "models",
            Self::Remove => "remove",
            Self::Disconnect => "disconnect",
        }
    }
}

#[derive(Clone)]
pub struct ProviderJobs {
    core: Core,
    pub active: Signal<Option<Job>>,
    pub notice: Signal<Option<String>>,
    pub reply: Signal<String>,
    epoch: Signal<u64>,
}

impl ProviderJobs {
    pub fn new(core: Core) -> Self {
        Self {
            core,
            active: signal(None),
            notice: signal(None),
            reply: signal(String::new()),
            epoch: signal(0),
        }
    }

    pub fn clear(&self) {
        self.notice.set(None);
        self.reply.set(String::new());
    }

    pub fn cancel_login(&self) -> bool {
        if self.active.get_untracked() != Some(Job::Login) {
            return false;
        }
        self.epoch.update(|epoch| *epoch += 1);
        close_auth_session();
        true
    }

    pub async fn run(&self, job: Job, id: &str, extra: Value) -> Result<Option<Value>, CoreError> {
        self.epoch.update(|epoch| *epoch += 1);
        let epoch = self.epoch.get_untracked();
        self.clear();
        self.active.set(Some(job));
        let result = self.start(job, id, extra, epoch).await;
        self.active.set(None);
        result
    }

    async fn start(
        &self,
        job: Job,
        id: &str,
        mut command: Value,
        epoch: u64,
    ) -> Result<Option<Value>, CoreError> {
        command["op"] = json!(if job == Job::Login {
            "ai_login"
        } else {
            "ai_start"
        });
        command["kind"] = json!(job.kind());
        command["id"] = json!(id);
        let started: Value = self.core.ai(command).await?;
        let task = started["taskId"]
            .as_str()
            .ok_or_else(|| problem("Could not start the provider request."))?;
        let result = self.poll(job, &started, task, epoch).await;
        if job == Job::Login {
            close_auth_session();
        }
        if !matches!(result, Ok(Some(_))) {
            let _: Result<Value, _> = self.core.ai(json!({"op":"ai_cancel","taskId":task})).await;
        }
        let _: Result<Value, _> = self.core.ai(json!({"op":"ai_forget","taskId":task})).await;
        if self.epoch.get_untracked() == epoch {
            result
        } else {
            Ok(None)
        }
    }

    async fn poll(
        &self,
        job: Job,
        started: &Value,
        task: &str,
        epoch: u64,
    ) -> Result<Option<Value>, CoreError> {
        let cancelled = signal(false);
        let mut opened = false;
        if self.epoch.get_untracked() == epoch && job == Job::Login && started["url"].is_string() {
            self.show_login(started, task, epoch, cancelled).await?;
            opened = true;
        }
        loop {
            if self.epoch.get_untracked() != epoch {
                return Ok(None);
            }
            let state: Value = self.core.ai(json!({"op":"ai_poll","taskId":task})).await?;
            if self.epoch.get_untracked() != epoch {
                return Ok(None);
            }
            if let Some(message) = state["message"].as_str() {
                self.notice.set(Some(message.into()));
            }
            if job == Job::Test
                && let Some(reply) = state["text"].as_str()
            {
                self.reply.set(reply.into());
            }
            match state["status"].as_str() {
                Some("completed") => {
                    let result = state["result"].clone();
                    let message = match job {
                        Job::Login if result["connected"] == true => {
                            "Account connected. Refresh models to see account access."
                        }
                        Job::Login if result["planEnabled"] == true => {
                            "Connected. Choose a model to use your ChatGPT plan."
                        }
                        Job::Login => {
                            "Account connected. ChatGPT plan usage was not granted; reconnect to enable it."
                        }
                        Job::Remove | Job::Disconnect if result["revoked"] == false => {
                            "Disconnected on this device. You can also revoke access in your provider’s account settings."
                        }
                        Job::Remove | Job::Disconnect => "Provider disconnected.",
                        _ => "",
                    };
                    self.notice
                        .set((!message.is_empty()).then(|| message.into()));
                    return Ok(Some(result));
                }
                Some("failed") => {
                    return Err(problem(
                        state["error"]
                            .as_str()
                            .unwrap_or("The provider request failed."),
                    ));
                }
                Some("cancelled") => return Ok(None),
                _ => {}
            }
            if cancelled.get_untracked() {
                return Err(problem(
                    "Sign-in was cancelled or returned an unexpected address.",
                ));
            }
            if job == Job::Login && !opened && state["login"].is_object() {
                self.show_login(&state["login"], task, epoch, cancelled)
                    .await?;
                opened = true;
            }
            timer(Duration::from_millis(250)).await;
        }
    }

    async fn show_login(
        &self,
        login: &Value,
        task: &str,
        epoch: u64,
        cancelled: Signal<bool>,
    ) -> Result<(), CoreError> {
        let url = login["url"]
            .as_str()
            .and_then(|url| Url::parse(url).ok())
            .filter(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
            })
            .ok_or_else(|| problem("The provider returned an invalid sign-in address."))?;
        if login["mode"] == "device" {
            let code = login["userCode"]
                .as_str()
                .filter(|code| !code.is_empty() && code.len() <= 100)
                .ok_or_else(|| problem("The provider did not return a sign-in code."))?;
            self.notice.set(Some(format!(
                "Enter code {code} in the browser, then return here."
            )));
            if !open_url(url.as_str()).await {
                return Err(problem("Could not open the sign-in page."));
            }
            return Ok(());
        }
        let expected = format!("govue://ai-login/{task}");
        if login["returnUrl"] != expected {
            return Err(problem(
                "The sign-in return address is missing or unexpected.",
            ));
        }
        let current = self.epoch;
        let session = open_auth_session(AuthSessionRequest::new(url.to_string(), "govue"));
        spawn(async move {
            let result = session.await;
            if current.get_untracked() == epoch
                && !matches!(result, Ok(AuthSessionOutcome::Completed(url)) if url == expected)
            {
                cancelled.set(true);
            }
        });
        Ok(())
    }
}

fn problem(message: &str) -> CoreError {
    CoreError::Request(message.into())
}
