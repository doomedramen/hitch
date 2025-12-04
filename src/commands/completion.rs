use crate::commands::global_context::GlobalContext;
use anyhow::Result;
use clap::Parser;

/// Generate shell completion scripts
#[derive(Parser)]
pub struct CompletionCommand {
    /// The shell to generate completions for
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

pub fn run(args: CompletionCommand, _context: &GlobalContext) -> Result<()> {
    // Use clap_complete to generate shell completions
    // Since we can't access the CLI struct from the lib crate, we'll generate manually
    let completion_script = match args.shell {
        clap_complete::Shell::Bash => generate_bash_completion(),
        clap_complete::Shell::Zsh => generate_zsh_completion(),
        clap_complete::Shell::Fish => generate_fish_completion(),
        clap_complete::Shell::PowerShell => generate_powershell_completion(),
        clap_complete::Shell::Elvish => generate_elvish_completion(),
        _ => "# Unsupported shell\n".to_string(),
    };

    print!("{}", completion_script);
    Ok(())
}

fn generate_bash_completion() -> String {
    r#"# bash completion for hitch
_hitch_completion() {
    local cur prev opts
    COMPREPLY=()
    cur="${COMP_WORDS[COMP_CWORD]}"
    prev="${COMP_WORDS[COMP_CWORD-1]}"
    opts="init add remove promote demote rebuild release status lock unlock guard completion --help --version --verbose --no-push"

    if [[ ${cur} == * ]] ; then
        COMPREPLY=( $(compgen -W "${opts}" -- ${cur}) )
        return 0
    fi
}

complete -F _hitch_completion hitch
"#.to_string()
}

fn generate_zsh_completion() -> String {
    r#"#compdef hitch

_hitch() {
    local -a commands
    commands=(
        'init:Initialize Hitch for environment branch management'
        'add:Add a new environment'
        'remove:Remove an environment from configuration'
        'promote:Promote a branch to an environment'
        'demote:Demote a branch from an environment'
        'rebuild:Rebuild environment by merging promoted branches'
        'release:Release environment branches to target branch'
        'status:Show status of environments and promoted branches'
        'lock:Lock environment to prevent deployments'
        'unlock:Unlock environment to allow deployments'
        'guard:Guard against direct commits to environment branches'
        'completion:Generate shell completion script'
    )

    if (( CURRENT == 2 )); then
        _describe 'command' commands
    elif (( CURRENT > 2 )); then
        local cmd=${words[2]}
        case $cmd in
            add|remove|promote|demote|lock|unlock)
                _arguments '1: :_guard "^-*" "environment name"'
                ;;
            promote|demote)
                if (( CURRENT == 3 )); then
                    _arguments '1: :_guard "^-*" "branch name"'
                elif (( CURRENT == 4 )); then
                    _arguments '1: :_guard "^-*" "environment name"'
                fi
                ;;
            rebuild|release|status)
                _arguments '1: :_guard "^-*" "environment name"'
                ;;
            guard)
                _arguments '1: :_guard "^-*" "branch name"'
                ;;
            completion)
                _arguments '1: :_guard "^-*" "shell (bash|zsh|fish|powershell|elvish)"'
                ;;
        esac
    fi
}
"#
    .to_string()
}

fn generate_fish_completion() -> String {
    r#"# fish completion for hitch

complete -c hitch -f

# Commands
complete -c hitch -n __fish_use_subcommand -a init -d "Initialize Hitch for environment branch management"
complete -c hitch -n __fish_use_subcommand -a add -d "Add a new environment"
complete -c hitch -n __fish_use_subcommand -a remove -d "Remove an environment from configuration"
complete -c hitch -n __fish_use_subcommand -a promote -d "Promote a branch to an environment"
complete -c hitch -n __fish_use_subcommand -a demote -d "Demote a branch from an environment"
complete -c hitch -n __fish_use_subcommand -a rebuild -d "Rebuild environment by merging promoted branches"
complete -c hitch -n __fish_use_subcommand -a release -d "Release environment branches to target branch"
complete -c hitch -n __fish_use_subcommand -a status -d "Show status of environments and promoted branches"
complete -c hitch -n __fish_use_subcommand -a lock -d "Lock environment to prevent deployments"
complete -c hitch -n __fish_use_subcommand -a unlock -d "Unlock environment to allow deployments"
complete -c hitch -n __fish_use_subcommand -a guard -d "Guard against direct commits to environment branches"
complete -c hitch -n __fish_use_subcommand -a completion -d "Generate shell completion script"

# Options
complete -c hitch -s h -l help -d "Print help"
complete -c hitch -l version -d "Print version"
complete -c hitch -l verbose -d "Print detailed logs"
complete -c hitch -l no-push -d "Skip automatic pushes"

# Command-specific arguments
complete -c hitch -n "__fish_seen_subcommand_from add remove promote demote lock unlock" -a "(hitch status 2>/dev/null | grep -E '^Environment:' | awk '{print \$2}')" -d "Environment"
complete -c hitch -n "__fish_seen_subcommand_from promote demote" -a "(git branch --format='%(refname:short)' 2>/dev/null)" -d "Branch"
complete -c hitch -n "__fish_seen_subcommand_from completion" -a "bash zsh fish powershell elvish" -d "Shell"
"#.to_string()
}

fn generate_powershell_completion() -> String {
    r#"# PowerShell completion for hitch

Register-ArgumentCompleter -Native -CommandName hitch -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commands = @(
        'init', 'add', 'remove', 'promote', 'demote',
        'rebuild', 'release', 'status', 'lock', 'unlock',
        'guard', 'completion'
    )

    if ($commandAst.CommandElements.Count -eq 2) {
        $commands | Where-Object { $_ -like "$wordToComplete*" } | ForEach-Object {
            [System.Management.Automation.CompletionResult]::new($_, $_, 'ParameterValue', $_)
        }
    }
}
"#
    .to_string()
}

fn generate_elvish_completion() -> String {
    r#"# elvish completion for hitch

edit:completion:hitch = [cmd]{
    put init add remove promote demote rebuild release status lock unlock guard completion
}
"#
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::Shell;

    #[test]
    fn test_completion_generation() {
        // Test that we can create the command
        let cmd = CompletionCommand { shell: Shell::Bash };
        assert_eq!(cmd.shell, Shell::Bash);
    }

    #[test]
    fn test_bash_completion_content() {
        let script = generate_bash_completion();

        // Check for key bash completion components
        assert!(script.contains("_hitch_completion()"));
        assert!(script.contains("complete -F _hitch_completion hitch"));
        assert!(script.contains("local cur prev opts"));
        assert!(script.contains("init add remove promote demote rebuild"));

        // Check options
        assert!(script.contains("--help --version --verbose --no-push"));
    }

    #[test]
    fn test_zsh_completion_content() {
        let script = generate_zsh_completion();

        // Check for key zsh completion components
        assert!(script.contains("#compdef hitch"));
        assert!(script.contains("_hitch"));
        assert!(script.contains("local -a commands"));

        // Check command definitions
        assert!(script.contains("'init:Initialize Hitch"));
        assert!(script.contains("'add:Add a new environment"));
        assert!(script.contains("'completion:Generate shell completion"));

        // Check argument handling
        assert!(script.contains("_hitch"));
        assert!(script.contains("_describe"));
    }

    #[test]
    fn test_fish_completion_content() {
        let script = generate_fish_completion();

        // Check for key fish completion components
        assert!(script.contains("complete -c hitch -f"));
        assert!(script.contains("__fish_use_subcommand"));
        assert!(script.contains("__fish_seen_subcommand_from"));

        // Check command completions
        assert!(script.contains("-a init -d \"Initialize Hitch"));
        assert!(script.contains("-a status -d \"Show status"));

        // Check conditional completions
        assert!(script.contains("hitch status 2>/dev/null"));
        assert!(script.contains("git branch --format="));
    }

    #[test]
    fn test_powershell_completion_content() {
        let script = generate_powershell_completion();

        // Check for key PowerShell completion components
        assert!(script.contains("Register-ArgumentCompleter"));
        assert!(script.contains("-CommandName hitch"));
        assert!(script.contains("ScriptBlock"));

        // Check command list
        assert!(script.contains("'init', 'add', 'remove'"));
        assert!(script.contains("'promote', 'demote'"));
        assert!(script.contains("'completion'"));

        // Check completion logic
        assert!(script.contains("CommandElements.Count"));
        assert!(script.contains("[System.Management.Automation.CompletionResult]::new"));
    }

    #[test]
    fn test_elvish_completion_content() {
        let script = generate_elvish_completion();

        // Check for key elvish completion components
        assert!(script.contains("edit:completion:hitch"));
        assert!(script.contains("put init add remove"));
        assert!(script.contains("promote demote rebuild"));

        // Check all commands are listed
        assert!(script.contains("status lock unlock"));
        assert!(script.contains("guard completion"));
    }

    #[test]
    fn test_all_shell_completions_generate() {
        let shells = vec![
            (Shell::Bash, generate_bash_completion()),
            (Shell::Zsh, generate_zsh_completion()),
            (Shell::Fish, generate_fish_completion()),
            (Shell::PowerShell, generate_powershell_completion()),
            (Shell::Elvish, generate_elvish_completion()),
        ];

        for (shell, script) in shells {
            assert!(
                !script.is_empty(),
                "Script for {:?} should not be empty",
                shell
            );
            assert!(
                script.len() > 100,
                "Script for {:?} should be substantial, got {} bytes",
                shell,
                script.len()
            );
        }
    }

    #[test]
    fn test_shell_consistency() {
        // Ensure all completion scripts mention the same set of commands
        let bash = generate_bash_completion();
        let zsh = generate_zsh_completion();
        let fish = generate_fish_completion();
        let powershell = generate_powershell_completion();
        let elvish = generate_elvish_completion();

        let core_commands = ["init", "add", "remove", "promote", "demote", "rebuild"];
        let extended_commands = ["status", "lock", "unlock", "guard", "completion"];

        for cmd in core_commands.iter().chain(extended_commands.iter()) {
            assert!(bash.contains(cmd), "bash missing: {}", cmd);
            assert!(zsh.contains(cmd), "zsh missing: {}", cmd);
            assert!(fish.contains(cmd), "fish missing: {}", cmd);
            assert!(powershell.contains(cmd), "powershell missing: {}", cmd);
            assert!(elvish.contains(cmd), "elvish missing: {}", cmd);
        }
    }

    #[test]
    fn test_completion_script_formatting() {
        // Ensure scripts have proper line endings and structure
        let scripts = vec![
            ("bash", generate_bash_completion()),
            ("zsh", generate_zsh_completion()),
            ("fish", generate_fish_completion()),
            ("powershell", generate_powershell_completion()),
            ("elvish", generate_elvish_completion()),
        ];

        for (name, script) in scripts {
            // Scripts should not have trailing whitespace on lines
            for line in script.lines() {
                assert!(
                    !line.ends_with(' '),
                    "Line in {} has trailing whitespace: {}",
                    name,
                    line
                );
            }

            // Scripts should end with a newline (most shells expect this)
            assert!(
                script.ends_with('\n'),
                "{} script should end with newline",
                name
            );
        }
    }
}
