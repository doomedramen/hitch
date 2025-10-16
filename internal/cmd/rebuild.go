package cmd

import (
	"fmt"
	"os/exec"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var (
	rebuildDryRun bool
	rebuildForce  bool
)

var rebuildCmd = &cobra.Command{
	Use:   "rebuild <environment>",
	Short: "Rebuild an environment from scratch",
	Long: `Rebuild an environment from scratch.

This command:
1. Acquires lock on environment
2. Checks out fresh base branch (main)
3. Creates temporary branch for safety
4. Merges all features into temp branch
5. Only if ALL merges succeed: swaps temp branch to become the new environment branch
6. Force-pushes rebuilt environment branch
7. Releases lock
8. Returns you to your original branch

Safety (always enabled):
- Original environment branch is never touched until rebuild succeeds
- If ANY merge fails, temp branch is deleted and original is preserved`,
	Args: cobra.ExactArgs(1),
	RunE: runRebuild,
}

func init() {
	rebuildCmd.Flags().BoolVar(&rebuildDryRun, "dry-run", false, "Simulate rebuild without making changes")
	rebuildCmd.Flags().BoolVar(&rebuildForce, "force", false, "Rebuild even if environment is locked")
	rootCmd.AddCommand(rebuildCmd)
}

func runRebuild(cmd *cobra.Command, args []string) error {
	envName := args[0]

	// 1. Open Git repository
	repo, err := hitchgit.OpenRepo(".")
	if err != nil {
		errorMsg("Not a Git repository")
		return err
	}

	// 2. Remember current branch
	currentBranch, err := repo.CurrentBranch()
	if err != nil {
		currentCommit, _ := repo.CurrentCommitSHA()
		currentBranch = currentCommit
	}

	// ALWAYS return to original branch
	defer func() {
		repo.Checkout(currentBranch)
	}()

	// 3. Read metadata
	reader := metadata.NewReader(repo.Repository)
	if !reader.Exists() {
		errorMsg("Hitch is not initialized in this repository")
		fmt.Println("\nRun 'hitch init' to initialize Hitch.")
		return fmt.Errorf("hitch not initialized")
	}

	meta, err := reader.Read()
	if err != nil {
		errorMsg("Failed to read metadata")
		return err
	}

	// 4. Validate environment exists
	env, exists := meta.Environments[envName]
	if !exists {
		errorMsg(fmt.Sprintf("Environment '%s' not found", envName))
		fmt.Println("\nAvailable environments:")
		for name := range meta.Environments {
			fmt.Printf("  - %s\n", name)
		}
		return fmt.Errorf("environment not found")
	}

	// 5. Get user info
	userEmail, err := repo.UserEmail()
	if err != nil {
		errorMsg("Git user.email is not configured")
		return err
	}

	// 6. Check/acquire lock
	if env.Locked && !rebuildForce {
		if env.LockedBy != userEmail {
			errorMsg(fmt.Sprintf("Environment '%s' is locked", envName))
			fmt.Println()
			fmt.Printf("Locked by: %s\n", env.LockedBy)
			fmt.Printf("Locked at: %s\n", env.LockedAt.Format("2006-01-02 15:04:05"))
			fmt.Println()

			if meta.IsLockStale(envName) {
				fmt.Printf("This lock is stale (older than %d minutes).\n", meta.Config.LockTimeoutMinutes)
				fmt.Printf("To force rebuild: hitch rebuild %s --force\n", envName)
			} else {
				fmt.Printf("Wait for unlock or contact %s\n", env.LockedBy)
			}

			return fmt.Errorf("environment locked")
		}
	}

	// Lock environment
	if !rebuildDryRun {
		if err := meta.LockEnvironment(envName, userEmail, "Rebuilding environment"); err != nil {
			errorMsg("Failed to acquire lock")
			return err
		}

		// Write metadata with lock
		writer := metadata.NewWriter(repo.Repository)
		userName, _ := repo.UserName()
		meta.UpdateMeta(userEmail, fmt.Sprintf("hitch rebuild %s", envName))
		if err := writer.Write(meta, fmt.Sprintf("Lock %s for rebuild", envName), userName, userEmail); err != nil {
			errorMsg("Failed to write metadata")
			return err
		}

		// Ensure unlock on exit
		defer func() {
			meta.UnlockEnvironment(envName)
			meta.UpdateMeta(userEmail, fmt.Sprintf("hitch rebuild %s (unlock)", envName))
			writer.Write(meta, fmt.Sprintf("Unlock %s after rebuild", envName), userName, userEmail)
		}()
	}

	// 7. Perform rebuild
	if rebuildDryRun {
		return performDryRunRebuild(repo, envName, env, meta)
	}

	return performRebuild(repo, envName, env, meta, userEmail)
}

func performRebuild(repo *hitchgit.Repo, envName string, env metadata.Environment, meta *metadata.Metadata, userEmail string) error {
	fmt.Printf("Rebuilding %s environment...\n\n", envName)

	baseBranch := env.Base
	tempBranch := envName + "-hitch-temp"

	// 1. Checkout and pull base branch
	success("Checked out base branch: " + baseBranch)
	if err := repo.Checkout(baseBranch); err != nil {
		errorMsg("Failed to checkout base branch")
		return err
	}

	// Pull latest (ignore errors if no remote)
	repo.Pull("origin", baseBranch)

	// 2. Create temp branch
	success("Created temp branch: " + tempBranch)

	// Delete temp branch if it exists
	repo.DeleteBranch(tempBranch, true)

	if err := repo.CreateBranch(tempBranch, baseBranch); err != nil {
		errorMsg("Failed to create temp branch")
		return err
	}

	if err := repo.Checkout(tempBranch); err != nil {
		errorMsg("Failed to checkout temp branch")
		return err
	}

	// 3. Merge all features
	if len(env.Features) == 0 {
		info("No features to merge")
	} else {
		fmt.Println("Merging features into temp branch:")
		for _, feature := range env.Features {
			if err := repo.Merge(feature, true); err != nil {
				// Merge failed!
				errorMsg(fmt.Sprintf("Merge conflict when adding %s", feature))
				fmt.Println()
				fmt.Printf("The branch %s conflicts with the current %s environment.\n", feature, envName)
				fmt.Println()
				fmt.Println("To resolve:")
				fmt.Printf("  1. git checkout %s\n", feature)
				fmt.Printf("  2. git rebase %s\n", baseBranch)
				fmt.Println("  3. Resolve conflicts and continue rebase")
				fmt.Println("  4. git push --force-with-lease")
				fmt.Printf("  5. hitch rebuild %s\n", envName)
				fmt.Println()

				// Cleanup
				repo.Checkout(baseBranch)
				repo.DeleteBranch(tempBranch, true)

				fmt.Println("✓ Original", envName, "branch is unchanged")
				fmt.Println("✓ Temp branch", tempBranch, "has been deleted")

				return fmt.Errorf("merge conflict")
			}
			success(fmt.Sprintf("  Merged %s (no conflicts)", feature))
		}
	}

	// 4. All merges succeeded! Swap branches
	success("All merges successful")

	// Checkout base to allow deleting env branch
	if err := repo.Checkout(baseBranch); err != nil {
		errorMsg("Failed to checkout base branch")
		return err
	}

	// Delete old environment branch
	if repo.BranchExists(envName) {
		if err := repo.DeleteBranch(envName, true); err != nil {
			warning(fmt.Sprintf("Failed to delete old %s branch: %v", envName, err))
		}
	}

	// Rename temp to env
	cmd := exec.Command("git", "branch", "-m", tempBranch, envName)
	if output, err := cmd.CombinedOutput(); err != nil {
		errorMsg("Failed to rename temp branch")
		return fmt.Errorf("rename failed: %s", string(output))
	}

	success(fmt.Sprintf("Swapped %s → %s", tempBranch, envName))

	// 5. Push to remote (ignore errors if no remote)
	if err := repo.Push("origin", envName, true); err != nil {
		warning("Failed to push to remote (this is OK if no remote configured)")
		fmt.Println("You may need to push manually:")
		fmt.Printf("  git push --force-with-lease origin %s\n", envName)
	} else {
		success("Pushed " + envName + " branch to remote")
	}

	fmt.Println()
	success(fmt.Sprintf("%s environment rebuilt with %d features", envName, len(env.Features)))

	return nil
}

func performDryRunRebuild(repo *hitchgit.Repo, envName string, env metadata.Environment, meta *metadata.Metadata) error {
	fmt.Printf("Dry run: simulating rebuild of %s environment\n\n", envName)

	baseBranch := env.Base
	tempBranch := envName + "-hitch-temp"

	info(fmt.Sprintf("Would checkout %s", baseBranch))
	info(fmt.Sprintf("Would create temp branch: %s", tempBranch))

	if len(env.Features) == 0 {
		info("No features to merge")
	} else {
		fmt.Println("Checking if features are mergeable:")
		for _, feature := range env.Features {
			// TODO: Actually check if merge would succeed
			info(fmt.Sprintf("  - %s (would merge)", feature))
		}
	}

	info(fmt.Sprintf("Would swap %s → %s", tempBranch, envName))
	info(fmt.Sprintf("Would push %s branch to remote", envName))

	fmt.Println()
	info("Dry run complete. No branches created, no changes made.")
	info("Run without --dry-run to apply changes.")

	return nil
}
