package cmd

import (
	"fmt"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var (
	promoteNoRebuild bool
)

var promoteCmd = &cobra.Command{
	Use:   "promote <branch> to <environment>",
	Short: "Add a feature branch to an environment",
	Long: `Add a feature branch to an environment.

This command:
1. Validates branch exists
2. Acquires lock on environment
3. Adds branch to environment's feature list
4. Rebuilds environment from base + all features (using safe temp branch)
5. Force-pushes rebuilt hitched branch
6. Updates metadata
7. Releases lock
8. Returns you to your original branch

Safety: Uses temporary branch for rebuild - original environment preserved until success!`,
	Args: cobra.ExactArgs(3), // branch, "to", environment
	RunE: runPromote,
}

func init() {
	promoteCmd.Flags().BoolVar(&promoteNoRebuild, "no-rebuild", false, "Add to metadata but don't rebuild")
	rootCmd.AddCommand(promoteCmd)
}

func runPromote(cmd *cobra.Command, args []string) error {
	if len(args) != 3 || args[1] != "to" {
		return fmt.Errorf("usage: hitch promote <branch> to <environment>")
	}

	branchName := args[0]
	envName := args[2]

	// 1. Open Git repository
	repo, err := hitchgit.OpenRepo(".")
	if err != nil {
		errorMsg("Not a Git repository")
		return err
	}

	// 2. Remember current branch (will return here at end)
	currentBranch, err := repo.CurrentBranch()
	if err != nil {
		currentCommit, _ := repo.CurrentCommitSHA()
		currentBranch = currentCommit
	}

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
	_, exists := meta.Environments[envName]
	if !exists {
		errorMsg(fmt.Sprintf("Environment '%s' not found", envName))
		fmt.Println("\nAvailable environments:")
		for name := range meta.Environments {
			fmt.Printf("  - %s\n", name)
		}
		return fmt.Errorf("environment not found")
	}

	// 5. Validate branch exists
	if !repo.BranchExists(branchName) {
		errorMsg(fmt.Sprintf("Branch '%s' not found", branchName))
		fmt.Println("\nMake sure the branch exists locally or remotely:")
		fmt.Printf("  git branch -a | grep %s\n", branchName)
		return fmt.Errorf("branch not found")
	}

	// 6. Get user info
	userEmail, err := repo.UserEmail()
	if err != nil {
		errorMsg("Git user.email is not configured")
		return err
	}

	userName, _ := repo.UserName()

	// 7. Check if already in environment
	env := meta.Environments[envName]
	for _, feature := range env.Features {
		if feature == branchName {
			warning(fmt.Sprintf("%s is already in %s", branchName, envName))
			return nil
		}
	}

	fmt.Printf("Promoting %s to %s...\n\n", branchName, envName)

	// 8. Add to metadata
	if err := meta.AddBranchToEnvironment(envName, branchName, userEmail); err != nil {
		errorMsg("Failed to add branch to environment")
		return err
	}

	success(fmt.Sprintf("Added %s to %s feature list", branchName, envName))

	// 9. Write metadata
	writer := metadata.NewWriter(repo.Repository)
	meta.UpdateMeta(userEmail, fmt.Sprintf("hitch promote %s to %s", branchName, envName))
	if err := writer.Write(meta, fmt.Sprintf("Promote %s to %s", branchName, envName), userName, userEmail); err != nil {
		errorMsg("Failed to write metadata")
		return err
	}

	success("Updated metadata")

	// 10. Rebuild environment (unless --no-rebuild)
	if promoteNoRebuild {
		fmt.Println()
		warning(fmt.Sprintf("Skipped rebuild (use 'hitch rebuild %s' to rebuild)", envName))
		return nil
	}

	fmt.Println()

	// Call rebuild command
	return runRebuildInternal(repo, envName, userEmail, userName, meta)
}

// runRebuildInternal is a helper that rebuilds without checking locks (caller handles locking)
func runRebuildInternal(repo *hitchgit.Repo, envName string, userEmail string, userName string, meta *metadata.Metadata) error {
	env := meta.Environments[envName]

	// Lock environment
	if err := meta.LockEnvironment(envName, userEmail, "Rebuilding after promote"); err != nil {
		// Check if we're the lock holder
		if !meta.IsLockedByUser(envName, userEmail) {
			errorMsg("Failed to acquire lock")
			return err
		}
	}

	// Write metadata with lock
	writer := metadata.NewWriter(repo.Repository)
	meta.UpdateMeta(userEmail, fmt.Sprintf("hitch rebuild %s (auto)", envName))
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

	// Perform rebuild
	return performRebuild(repo, envName, env, meta, userEmail)
}
