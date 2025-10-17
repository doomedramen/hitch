package cmd

import (
	"fmt"
	"os"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var hookCmd = &cobra.Command{
	Use:   "hook <hook-name>",
	Short: "Git hook integration commands",
	Long: `Git hook integration commands for use in your Git hooks.

Available hooks:
  pre-push - Check if current branch is safe to push

These commands are designed to be called from Git hooks (.git/hooks/).
They provide exit codes suitable for hook integration.

Exit codes:
  0 - Safe to proceed
  1 - Blocked (hook should abort operation)`,
}

var hookPrePushCmd = &cobra.Command{
	Use:   "pre-push",
	Short: "Check if current branch can be safely pushed",
	Long: `Check if the current branch can be safely pushed.

Checks:
1. Is current branch a managed environment (dev, qa)?
2. Is the environment locked?
3. If locked, am I the lock holder?

Usage in .git/hooks/pre-push:
  #!/bin/bash
  hitch hook pre-push || exit 1`,
	RunE: runHookPrePush,
}

func init() {
	hookCmd.AddCommand(hookPrePushCmd)
}

func runHookPrePush(cmd *cobra.Command, args []string) error {
	// 1. Open Git repository
	repo, err := hitchgit.OpenRepo(".")
	if err != nil {
		// Not a git repo, allow push
		os.Exit(0)
		return nil
	}

	// 2. Check if Hitch is initialized
	reader := metadata.NewReader(repo.Repository)
	if !reader.Exists() {
		// Hitch not initialized, allow push
		os.Exit(0)
		return nil
	}

	// 3. Get current branch
	currentBranch, err := repo.CurrentBranch()
	if err != nil {
		// Detached HEAD or error, allow push
		os.Exit(0)
		return nil
	}

	// 4. Read metadata
	meta, err := reader.Read()
	if err != nil {
		// Can't read metadata, fail open (allow push)
		os.Exit(0)
		return nil
	}

	// 5. Check if current branch is a managed environment
	env, isManaged := meta.Environments[currentBranch]
	if !isManaged {
		// Not a hitched branch, allow push
		os.Exit(0)
		return nil
	}

	// 6. Get current user
	userEmail, err := repo.UserEmail()
	if err != nil {
		userEmail = ""
	}

	// 7. Check if locked
	if env.Locked {
		if env.LockedBy != userEmail {
			// Locked by someone else, block push
			errorMsg(fmt.Sprintf("Cannot push to %s", currentBranch))
			fmt.Fprintln(os.Stderr)
			fmt.Fprintf(os.Stderr, "Locked by: %s\n", env.LockedBy)
			fmt.Fprintf(os.Stderr, "Locked at: %s\n", env.LockedAt.Format("2006-01-02 15:04:05"))
			fmt.Fprintln(os.Stderr)

			if meta.IsLockStale(currentBranch) {
				fmt.Fprintf(os.Stderr, "This lock is stale (older than %d minutes).\n", meta.Config.LockTimeoutMinutes)
				fmt.Fprintln(os.Stderr)
				fmt.Fprintf(os.Stderr, "To force unlock: hitch unlock %s --force\n", currentBranch)
			} else {
				fmt.Fprintf(os.Stderr, "Wait for unlock or contact %s\n", env.LockedBy)
			}

			os.Exit(1)
			return nil
		}

		// Locked by current user, allow push (probably hitch operation)
		os.Exit(0)
		return nil
	}

	// Not locked but is hitched branch, warn but allow
	warning(fmt.Sprintf("Pushing directly to hitched branch %s", currentBranch))
	fmt.Fprintf(os.Stderr, "This may be overwritten by: hitch rebuild %s\n", currentBranch)
	fmt.Fprintln(os.Stderr)

	os.Exit(0)
	return nil
}
