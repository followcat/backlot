// Derived from herdr src/config/sound.rs (Apache-2.0).
//! `[ui.sound]` parsing: whether synthesized notification sounds fire,
//! per-agent overrides, and the reference's mp3 path fields (retained for
//! config compatibility; hc-tui signals sound with the host bell instead
//! of an audio player — see docs/parity.md).

use std::path::PathBuf;

use serde::Deserialize;

use super::agent::Agent;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct SoundConfig {
    pub enabled: bool,
    /// Optional mp3 file path used for all notification sounds.
    /// Relative paths are resolved from the config file's directory.
    pub path: Option<PathBuf>,
    /// Optional mp3 file path for "done" notifications.
    /// Relative paths are resolved from the config file's directory.
    pub done_path: Option<PathBuf>,
    /// Optional mp3 file path for "request" notifications.
    /// Relative paths are resolved from the config file's directory.
    pub request_path: Option<PathBuf>,
    pub agents: AgentSoundOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct AgentSoundOverrides {
    pub pi: AgentSoundSetting,
    pub claude: AgentSoundSetting,
    pub codex: AgentSoundSetting,
    pub gemini: AgentSoundSetting,
    pub cursor: AgentSoundSetting,
    pub devin: AgentSoundSetting,
    pub agy: AgentSoundSetting,
    pub cline: AgentSoundSetting,
    pub open_code: AgentSoundSetting,
    pub github_copilot: AgentSoundSetting,
    pub kimi: AgentSoundSetting,
    pub kiro: AgentSoundSetting,
    pub droid: AgentSoundSetting,
    pub amp: AgentSoundSetting,
    pub grok: AgentSoundSetting,
    pub hermes: AgentSoundSetting,
    pub kilo: AgentSoundSetting,
    pub qodercli: AgentSoundSetting,
    pub qwen: AgentSoundSetting,
    pub letta: AgentSoundSetting,
    pub maki: AgentSoundSetting,
    pub muse: AgentSoundSetting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentSoundSetting {
    #[default]
    Default,
    On,
    Off,
}

impl SoundConfig {
    /// The reference `allows`: sound fires unless disabled globally or for
    /// this agent. `None` (unidentified agent) takes the default setting.
    pub fn allows(&self, agent: Option<Agent>) -> bool {
        if !self.enabled {
            return false;
        }
        !matches!(self.agents.for_agent(agent), AgentSoundSetting::Off)
    }
}

impl AgentSoundOverrides {
    pub fn for_agent(&self, agent: Option<Agent>) -> AgentSoundSetting {
        match agent {
            Some(Agent::Pi) => self.pi,
            Some(Agent::Claude) => self.claude,
            Some(Agent::Codex) => self.codex,
            Some(Agent::Gemini) => self.gemini,
            Some(Agent::Cursor) => self.cursor,
            Some(Agent::Devin) => self.devin,
            Some(Agent::Antigravity) => self.agy,
            Some(Agent::Cline) => self.cline,
            Some(Agent::Omp) | Some(Agent::Mastracode) => AgentSoundSetting::Default,
            Some(Agent::OpenCode) => self.open_code,
            Some(Agent::GithubCopilot) => self.github_copilot,
            Some(Agent::Kimi) => self.kimi,
            Some(Agent::Kiro) => self.kiro,
            Some(Agent::Droid) => self.droid,
            Some(Agent::Amp) => self.amp,
            Some(Agent::Grok) => self.grok,
            Some(Agent::Hermes) => self.hermes,
            Some(Agent::Kilo) => self.kilo,
            Some(Agent::Qodercli) => self.qodercli,
            Some(Agent::Qwen) => self.qwen,
            Some(Agent::Letta) => self.letta,
            Some(Agent::Maki) => self.maki,
            Some(Agent::Muse) => self.muse,
            None => AgentSoundSetting::Default,
        }
    }
}

impl Default for SoundConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
            done_path: None,
            request_path: None,
            agents: AgentSoundOverrides::default(),
        }
    }
}

impl Default for AgentSoundOverrides {
    fn default() -> Self {
        Self {
            pi: AgentSoundSetting::Default,
            claude: AgentSoundSetting::Default,
            codex: AgentSoundSetting::Default,
            gemini: AgentSoundSetting::Default,
            cursor: AgentSoundSetting::Default,
            devin: AgentSoundSetting::Default,
            agy: AgentSoundSetting::Default,
            cline: AgentSoundSetting::Default,
            open_code: AgentSoundSetting::Default,
            github_copilot: AgentSoundSetting::Default,
            kimi: AgentSoundSetting::Default,
            kiro: AgentSoundSetting::Default,
            droid: AgentSoundSetting::Off,
            amp: AgentSoundSetting::Default,
            grok: AgentSoundSetting::Default,
            hermes: AgentSoundSetting::Default,
            kilo: AgentSoundSetting::Default,
            qodercli: AgentSoundSetting::Default,
            qwen: AgentSoundSetting::Default,
            letta: AgentSoundSetting::Default,
            maki: AgentSoundSetting::Default,
            muse: AgentSoundSetting::Default,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn sound_table_config_parses() {
        let config: crate::config::Config = toml::from_str(
            r#"
[ui.sound]
enabled = true
path = "sounds/all.mp3"
done_path = "sounds/done.mp3"
request_path = "/tmp/request.mp3"

[ui.sound.agents]
droid = "off"
claude = "on"
"#,
        )
        .unwrap();
        assert!(config.ui.sound.enabled);
        assert_eq!(config.ui.sound.path, Some(PathBuf::from("sounds/all.mp3")));
        assert_eq!(
            config.ui.sound.done_path,
            Some(PathBuf::from("sounds/done.mp3"))
        );
        assert_eq!(
            config.ui.sound.request_path,
            Some(PathBuf::from("/tmp/request.mp3"))
        );
        assert_eq!(config.ui.sound.agents.droid, AgentSoundSetting::Off);
        assert_eq!(config.ui.sound.agents.claude, AgentSoundSetting::On);
        assert_eq!(config.ui.sound.agents.pi, AgentSoundSetting::Default);
    }

    #[test]
    fn allows_honors_enabled_and_agent_overrides() {
        let sound = SoundConfig::default();
        assert!(sound.allows(None));
        assert!(sound.allows(Some(Agent::Claude)));
        assert!(!sound.allows(Some(Agent::Droid)));
        let silent = SoundConfig {
            enabled: false,
            ..SoundConfig::default()
        };
        assert!(!silent.allows(Some(Agent::Claude)));
    }
}
