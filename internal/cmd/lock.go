package cmd

import (
	"fmt"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var (
	lockReason string
	lockForce  bool
)

var lockCmd = &cobra.Command{
	Use:   "lock <environment>",
	Short: "Lock an environment to prevent modifications",
	Long: `Lock an environment to prevent other users from modifying it.

Locked environments cannot be rebuilt or have features promoted/demoted
until they are unlocked.

Example:
  hitch lock dev --reason "Testing critical fix"`,
	Args: cobra.ExactArgs(1),
	RunE: runLock,
}

func init() {
	lockCmd.Flags().StringVarP(&lockReason, "reason", "r", "", "Reason for locking")
	lockCmd.Flags().BoolVarP(&lockForce, "force", "f", false, "Force lock even if stale lock exists")
	rootCmd.AddCommand(lockCmd)
}

func runLock(cmd *cobra.Command, args []string) error {
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
	_, exists := meta.Environments[envName]
	if !exists {
		errorMsg(fmt.Sprintf("Environment '%s' not found", envName))
		return fmt.Errorf("environment not found")
	}

	// 6. Check for stale lock
	if meta.IsEnvironmentLocked(envName) && !lockForce {
		env := meta.Environments[envName]
		if meta.IsLockStale(envName) {
			warning(fmt.Sprintf("Environment '%s' has a stale lock (locked by %s)", envName, env.LockedBy))
			fmt.Println("Use --force to override the stale lock")
			return fmt.Errorf("stale lock exists")
		}
	}

	// 7. Lock environment
	if err := meta.LockEnvironment(envName, userEmail, lockReason); err != nil {
		errorMsg(fmt.Sprintf("Failed to lock environment: %v", err))
		return err
	}

	// 8. Update metadata
	meta.UpdateMeta(userEmail, fmt.Sprintf("hitch lock %s", envName))

	writer := metadata.NewWriter(repo.Repository)
	if err := writer.Write(meta, userName, userEmail, fmt.Sprintf("Lock %s environment", envName)); err != nil {
		errorMsg("Failed to update metadata")
		return err
	}

	success(fmt.Sprintf("Locked %s environment", envName))
	if lockReason != "" {
		fmt.Printf("Reason: %s\n", lockReason)
	}

	return nil
}
