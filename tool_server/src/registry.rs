use std::collections::HashMap;

use crate::tools::{all_tools, ServerTool};

pub struct ServerToolRegistry {
    tools: HashMap<String, ServerTool>,
}

impl ServerToolRegistry {
    pub fn new() -> Self {
        let mut tools = HashMap::new();

        for tool in all_tools() {
            tools.insert(tool.name.clone(), tool);
        }

        Self { tools }
    }

    pub fn get(&self, tool_name: &str) -> Option<&ServerTool> {
        self.tools.get(tool_name)
    }

    pub fn all(&self) -> impl Iterator<Item = &ServerTool> {
        self.tools.values()
    }

    pub fn insert(&mut self, tool: ServerTool) {
        self.tools.insert(tool.name.clone(), tool);
    }
}
