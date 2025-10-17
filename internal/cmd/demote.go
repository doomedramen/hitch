package cmd

import (
	"fmt"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var (
	demoteNoRebuild bool
)

var demoteCmd = &cobra.Command{
	Use:   "demote <branch> from <environment>",
	Short: "Remove a feature branch from an environment",
	Long: `Remove a feature branch from an environment.

This command:
1. Acquires lock on environment
2. Removes branch from environment's feature list
3. Rebuilds environment without that branch
4. Force-pushes rebuilt hitched branch
5. Updates metadata
6. Releases lock`,
	Args: cobra.ExactArgs(3), // branch, "from", environment
	RunE: runDemote,
}

func init() {
	demoteCmd.Flags().BoolVar(&demoteNoRebuild, "no-rebuild", false, "Remove from metadata but don't rebuild")
	rootCmd.AddCommand(demoteCmd)
}

func runDemote(cmd *cobra.Command, args []string) error {
	if len(args) != 3 || args[1] != "from" {
		return fmt.Errorf("usage: hitch demote <branch> from <environment>")
	}

	branchName := args[0]
	envName := args[2]

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

	defer func() {
		repo.Checkout(currentBranch)
	}()

	// 3. Read metadata
	reader := metadata.NewReader(repo.Repository)
	if !reader.Exists() {
		errorMsg("Hitch is not initialized in this repository")
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
		return fmt.Errorf("environment not found")
	}

	// 5. Get user info
	userEmail, err := repo.UserEmail()
	if err != nil {
		errorMsg("Git user.email is not configured")
		return err
	}

	userName, _ := repo.UserName()

	fmt.Printf("Demoting %s from %s...\n\n", branchName, envName)

	// 6. Remove from metadata
	if err := meta.RemoveBranchFromEnvironment(envName, branchName, userEmail); err != nil {
		errorMsg("Failed to remove branch from environment")
		return err
	}

	success(fmt.Sprintf("Removed %s from %s feature list", branchName, envName))

	// 7. Write metadata
	writer := metadata.NewWriter(repo.Repository)
	meta.UpdateMeta(userEmail, fmt.Sprintf("hitch demote %s from %s", branchName, envName))
	if err := writer.Write(meta, fmt.Sprintf("Demote %s from %s", branchName, envName), userName, userEmail); err != nil {
		errorMsg("Failed to write metadata")
		return err
	}

	success("Updated metadata")

	// 8. Rebuild environment (unless --no-rebuild)
	if demoteNoRebuild {
		fmt.Println()
		warning(fmt.Sprintf("Skipped rebuild (use 'hitch rebuild %s' to rebuild)", envName))
		return nil
	}

	fmt.Println()

	// Rebuild
	return runRebuildInternal(repo, envName, userEmail, userName, meta)
}
