use std::collections::HashMap;

use crate::tool_server::tools::{all_tools, ServerTool};

pub struct ServerToolRegistry {
    tools: HashMap<&'static str, ServerTool>,
}

impl ServerToolRegistry {
    pub fn new() -> Self {
        let mut tools = HashMap::new();

        for tool in all_tools() {
            tools.insert(tool.name, tool);
        }

        Self { tools }
    }

    pub fn get(&self, tool_name: &str) -> Option<&ServerTool> {
        self.tools.get(tool_name)
    }

    pub fn all(&self) -> impl Iterator<Item = &ServerTool> {
        self.tools.values()
    }
}
