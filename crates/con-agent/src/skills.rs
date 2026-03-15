use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A skill is a named capability with a prompt template.
/// Skills are loaded from AGENTS.md or registered programmatically.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub prompt_template: String,
    /// Source: "builtin", "agents_md", "plugin"
    pub source: String,
}

/// Registry of available skills
#[derive(Debug, Clone, Default)]
pub struct SkillRegistry {
    skills: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn new() -> Self {
        let mut registry = Self::default();
        registry.register_builtins();
        registry
    }

    fn register_builtins(&mut self) {
        self.register(Skill {
            name: "explain".to_string(),
            description: "Explain the last command output or error".to_string(),
            prompt_template: "Explain this terminal output. What happened and why? If there's an error, suggest how to fix it.".to_string(),
            source: "builtin".to_string(),
        });

        self.register(Skill {
            name: "fix".to_string(),
            description: "Fix the last error".to_string(),
            prompt_template: "The last command failed. Analyze the error, determine the root cause, and execute the fix.".to_string(),
            source: "builtin".to_string(),
        });

        self.register(Skill {
            name: "commit".to_string(),
            description: "Create a git commit with a good message".to_string(),
            prompt_template: "Look at the staged changes (run `git diff --cached`), write a concise commit message following conventional commits, and create the commit.".to_string(),
            source: "builtin".to_string(),
        });

        self.register(Skill {
            name: "test".to_string(),
            description: "Run tests and fix failures".to_string(),
            prompt_template: "Run the project's test suite. If any tests fail, analyze the failures and fix them.".to_string(),
            source: "builtin".to_string(),
        });

        self.register(Skill {
            name: "review".to_string(),
            description: "Review recent changes".to_string(),
            prompt_template: "Review the recent changes (git diff). Look for bugs, security issues, and suggest improvements.".to_string(),
            source: "builtin".to_string(),
        });
    }

    pub fn register(&mut self, skill: Skill) {
        self.skills.insert(skill.name.clone(), skill);
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn list(&self) -> Vec<&Skill> {
        self.skills.values().collect()
    }

    pub fn names(&self) -> Vec<String> {
        self.skills.keys().cloned().collect()
    }

    /// Load skills from an AGENTS.md file.
    /// AGENTS.md format:
    /// ```text
    /// ## skill-name
    /// Description of what this skill does.
    ///
    /// ```prompt
    /// The prompt template for the agent.
    /// ```
    /// ```
    pub fn load_agents_md(&mut self, path: &Path) -> anyhow::Result<usize> {
        let content = std::fs::read_to_string(path)?;
        let mut loaded = 0;

        let mut current_name: Option<String> = None;
        let mut current_desc = String::new();
        let mut current_prompt = String::new();
        let mut in_prompt_block = false;

        for line in content.lines() {
            if line.starts_with("## ") {
                // Save previous skill if any
                if let Some(name) = current_name.take() {
                    if !current_prompt.is_empty() {
                        self.register(Skill {
                            name,
                            description: current_desc.trim().to_string(),
                            prompt_template: current_prompt.trim().to_string(),
                            source: "agents_md".to_string(),
                        });
                        loaded += 1;
                    }
                }
                current_name = Some(line[3..].trim().to_string());
                current_desc = String::new();
                current_prompt = String::new();
                in_prompt_block = false;
            } else if line.starts_with("```prompt") {
                in_prompt_block = true;
            } else if line.starts_with("```") && in_prompt_block {
                in_prompt_block = false;
            } else if in_prompt_block {
                current_prompt.push_str(line);
                current_prompt.push('\n');
            } else if current_name.is_some() && !in_prompt_block && !line.starts_with('#') {
                current_desc.push_str(line);
                current_desc.push('\n');
            }
        }

        // Save last skill
        if let Some(name) = current_name {
            if !current_prompt.is_empty() {
                self.register(Skill {
                    name,
                    description: current_desc.trim().to_string(),
                    prompt_template: current_prompt.trim().to_string(),
                    source: "agents_md".to_string(),
                });
                loaded += 1;
            }
        }

        Ok(loaded)
    }
}
