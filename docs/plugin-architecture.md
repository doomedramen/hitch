# Plugin Architecture Research for Hitch

## Overview

This document explores possibilities for adding plugin support to Hitch, allowing users and organizations to extend its functionality beyond the core Git-based environment management.

## Potential Plugin Use Cases

### 1. **Custom Validators**
- Validate branch names against company conventions
- Enforce code review requirements before promotion
- Check if CI/CD pipelines have passed

### 2. **Notification Hooks**
- Slack/Teams notifications on environment changes
- Email alerts for deployments
- JIRA ticket creation on promote/demote

### 3. **External Integrations**
- Update deployment tracking systems
- Trigger external build pipelines
- Sync with project management tools

### 4. **Custom Commands**
- Organization-specific workflows
- Integration with internal tools
- Custom reporting formats

## Architecture Options

### Option 1: Hook-Based System (Recommended)

Similar to Git hooks, Hitch could provide lifecycle hooks that plugins can register for:

```rust
pub trait HitchPlugin {
    fn name(&self) -> &str;
    fn version(&self) -> &str;

    // Lifecycle hooks
    fn on_before_promote(&self, context: &HookContext) -> Result<HookResult>;
    fn on_after_promote(&self, context: &HookContext) -> Result<HookResult>;
    fn on_before_rebuild(&self, context: &HookContext) -> Result<HookResult>;
    fn on_after_rebuild(&self, context: &HookContext) -> Result<HookResult>;
    // ... more hooks
}

pub struct HookContext {
    pub environment: String,
    pub branch: String,
    pub config: HitchConfig,
    pub metadata: HashMap<String, serde_json::Value>,
}

pub enum HookResult {
    Continue,
    Stop(String), // Stop with error message
    Skip(String),  // Skip operation with reason
}
```

**Pros:**
- Simple to understand
- Minimal changes to core code
- Easy to test individual plugins
- Good performance (no runtime discovery)

**Cons:**
- Requires recompilation to add plugins
- Plugin configuration is static

### Option 2: Dynamic Plugin System

Use dynamic libraries (.so/.dll/.dylib) that can be loaded at runtime:

```rust
pub struct PluginManager {
    plugins: Vec<Box<dyn HitchPlugin>>,
}

impl PluginManager {
    pub fn load_plugin<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        // Load dynamic library
        // Find required symbols
        // Create plugin instance
    }

    pub fn execute_hooks(&self, hook_type: HookType, context: &HookContext) -> Vec<HookResult> {
        // Execute all plugins that registered for this hook
    }
}
```

**Pros:**
- No recompilation needed for new plugins
- Third-party plugins possible
- Dynamic enable/disable

**Cons:**
- Complex implementation
- Security concerns (loading arbitrary code)
- Platform-specific implementation
- Harder distribution

### Option 3: WASM-Based Plugins

Use WebAssembly for sandboxed plugin execution:

```rust
use wasmtime::*;

pub struct WasmPlugin {
    engine: Engine,
    module: Module,
    store: Store<PluginState>,
}

impl HitchPlugin for WasmPlugin {
    fn on_before_promote(&self, context: &HookContext) -> Result<HookResult> {
        // Call Wasm function with serialized context
        let result = self.call_function("on_before_promote", &context)?;
        // Parse and return result
    }
}
```

**Pros:**
- Sandboxed execution (secure)
- Cross-platform
- Multiple language support (Rust, JS, Go, etc.)
- Deterministic execution

**Cons:**
- Performance overhead
- Complex setup
- Limited access to system resources
- Wasm ecosystem maturity

### Option 4: Configuration-Driven Actions

Define actions in configuration files that can execute external commands:

```yaml
# ~/.hitch/plugins.yaml
plugins:
  - name: slack-notifier
    on_after_promote:
      - command: slack-notify
        args:
          - --channel
          - "#deployments"
          - --message
          - "Branch ${branch} promoted to ${environment}"

  - name: ci-validator
    on_before_promote:
      - command: ci-check
        args:
          - --branch
          - "${branch}"
        required: true
```

**Pros:**
- Simple to implement
- Users don't need to write code
- Easy to understand and debug

**Cons:**
- Limited flexibility
- Security risks (command injection)
- No proper error handling

## Recommended Implementation Plan

### Phase 1: Hook Infrastructure (Immediate)

1. **Define Hook Interfaces**
   ```rust
   pub mod hooks {
       pub trait HitchPlugin {
           fn name(&self) -> &str;
           fn version(&self) -> &str;
           fn execute(&self, hook: HookType, context: &HookContext) -> HookResult;
       }

       pub enum HookType {
           BeforePromote,
           AfterPromote,
           BeforeDemote,
           AfterDemote,
           BeforeRebuild,
           AfterRebuild,
           BeforeLock,
           AfterUnlock,
       }
   }
   ```

2. **Add Plugin Registration**
   ```rust
   // In src/lib.rs or main.rs
   pub fn register_plugins(registry: &mut PluginRegistry) {
       registry.register(Box::new(DefaultPlugins::new()));
   }

   // Built-in plugins in src/plugins/
   pub struct DefaultPlugins;
   impl HitchPlugin for DefaultPlugins {
       // Implement basic validation and notification hooks
   }
   ```

3. **Integrate into Commands**
   ```rust
   // In commands/promote.rs
   let hook_context = HookContext {
       environment: env_name.to_string(),
       branch: branch_name.to_string(),
       config: config.clone(),
       metadata: metadata.clone(),
   };

   // Execute before hooks
   for result in context.plugin_manager.execute_hooks(HookType::BeforePromote, &hook_context) {
       match result {
           HookResult::Stop(msg) => return Err(anyhow::anyhow!(msg)),
           HookResult::Skip(msg) => {
               context.log_info(&format!("Skipping promotion: {}", msg));
               return Ok(());
           }
           HookResult::Continue => continue,
       }
   }

   // ... perform promotion ...

   // Execute after hooks
   context.plugin_manager.execute_hooks(HookType::AfterPromote, &hook_context);
   ```

### Phase 2: Plugin Discovery (Future)

1. **Directory-based Discovery**
   - Look for plugins in `~/.hitch/plugins/`
   - Support for multiple plugin formats (Rust modules, WASM, scripts)

2. **Configuration System**
   ```toml
   # ~/.hitch/config.toml
   [plugins]
   enabled = ["slack-notifier", "ci-validator"]

   [plugins.slack-notifier]
   webhook_url = "https://hooks.slack.com/..."

   [plugins.ci-validator]
   jenkins_url = "https://ci.example.com/"
   ```

### Phase 3: External Plugin Support (Long-term)

1. **Plugin SDK**
   - Crate for writing plugins
   - Documentation and examples
   - Testing utilities

2. **Plugin Registry**
   - Official repository for community plugins
   - Version compatibility matrix
   - Security review process

## Security Considerations

1. **Code Review**
   - All plugins should be reviewable
   - Clear license requirements
   - Security guidelines

2. **Sandboxing**
   - Limit file system access
   - Control network access
   - Resource limits

3. **Validation**
   - Plugin metadata validation
   - Signature verification
   - Dependency scanning

## Implementation Complexity

| Feature | Complexity | Time Estimate |
|---------|------------|---------------|
| Hook Infrastructure | Low | 1-2 weeks |
| Built-in Plugins | Medium | 2-3 weeks |
| Configuration System | Medium | 1-2 weeks |
| WASM Support | High | 4-6 weeks |
| Dynamic Loading | High | 3-4 weeks |
| Plugin SDK | Low | 1 week |

## Recommendation

Start with **Option 1 (Hook-Based System)** for the following reasons:

1. **Immediate Value**: Even basic hooks add significant value
2. **Low Risk**: Doesn't compromise security or stability
3. **Incremental**: Can be extended later without breaking changes
4. **Familiar**: Similar to existing Git hooks model

The initial implementation should include:
- Core hook interfaces
- Plugin registration system
- A few built-in plugins (validation, notification)
- Integration points in key commands

This provides a solid foundation that can be evolved based on user feedback and actual use cases.