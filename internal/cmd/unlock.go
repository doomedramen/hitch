package cmd

import (
	"fmt"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var unlockForce bool

var unlockCmd = &cobra.Command{
	Use:   "unlock <environment>",
	Short: "Unlock an environment",
	Long: `Unlock an environment to allow modifications.

By default, you can only unlock environments that you locked yourself.
Use --force to unlock environments locked by others (requires admin).

Example:
  hitch unlock dev`,
	Args: cobra.ExactArgs(1),
	RunE: runUnlock,
}

func init() {
	unlockCmd.Flags().BoolVarP(&unlockForce, "force", "f", false, "Force unlock even if locked by another user")
	rootCmd.AddCommand(unlockCmd)
}

func runUnlock(cmd *cobra.Command, args []string) error {
	envName := args[0]

	// 1. Open Git repository
	repo, err := hitchgit.OpenRepo(".")
	if err != nil {
		errorMsg("Not a Git repository")
		return err
	}

	// 2. Get current branch to return to
	currentBranch, err := repo.CurrentBranch()
	if err != nil {
		errorMsg("Failed to get current branch")
		return err
	}
	defer func() {
		_ = repo.Checkout(currentBranch)
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

	// 4. Get user info
	userEmail, err := repo.UserEmail()
	if err != nil {
		errorMsg("Failed to get user email from git config")
		return err
	}
	userName, err := repo.UserName()
	if err != nil {
		errorMsg("Failed to get user name from git config")
		return err
	}

	// 5. Check if environment exists
	env, exists := meta.Environments[envName]
	if !exists {
		errorMsg(fmt.Sprintf("Environment '%s' not found", envName))
		return fmt.Errorf("environment not found")
	}

	// 6. Check if locked
	if !env.Locked {
		warning(fmt.Sprintf("Environment '%s' is not locked", envName))
		return nil
	}

	// 7. Check permissions
	if env.LockedBy != userEmail && !unlockForce {
		errorMsg(fmt.Sprintf("Environment '%s' is locked by %s", envName, env.LockedBy))
		fmt.Println("You can only unlock environments you locked yourself.")
		fmt.Println("Use --force to override (admin only)")
		return fmt.Errorf("permission denied")
	}

	// 8. Unlock environment
	if err := meta.UnlockEnvironment(envName); err != nil {
		errorMsg(fmt.Sprintf("Failed to unlock environment: %v", err))
		return err
	}

	// 9. Update metadata
	meta.UpdateMeta(userEmail, fmt.Sprintf("hitch unlock %s", envName))

	writer := metadata.NewWriter(repo.Repository)
	if err := writer.Write(meta, userName, userEmail, fmt.Sprintf("Unlock %s environment", envName)); err != nil {
		errorMsg("Failed to update metadata")
		return err
	}

	success(fmt.Sprintf("Unlocked %s environment", envName))

	return nil
}
