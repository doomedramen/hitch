package cmd

import (
	"fmt"
	"time"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var (
	releaseNoDelete bool
	releaseMessage  string
	releaseSquash   bool
)

var releaseCmd = &cobra.Command{
	Use:   "release <branch>",
	Short: "Merge a feature branch to the base branch (main)",
	Long: `Merge a feature branch to the base branch (typically main).

This command:
1. Validates branch is in at least one environment (safety check)
2. Merges branch into base branch (main)
3. Pushes base branch to remote
4. Removes branch from all environments
5. Records merge timestamp in metadata
6. Marks branch for cleanup after retention period

Safety: Ensures feature has been tested in at least one environment before release.`,
	Args: cobra.ExactArgs(1),
	RunE: runRelease,
}

func init() {
	releaseCmd.Flags().BoolVar(&releaseNoDelete, "no-delete", false, "Don't mark branch for cleanup after merge")
	releaseCmd.Flags().StringVar(&releaseMessage, "message", "", "Custom merge commit message")
	releaseCmd.Flags().BoolVar(&releaseSquash, "squash", false, "Squash commits before merging")
	rootCmd.AddCommand(releaseCmd)
}

func runRelease(cmd *cobra.Command, args []string) error {
	branchName := args[0]

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

	// 4. Validate branch exists in metadata
	branchInfo, exists := meta.Branches[branchName]
	if !exists {
		errorMsg(fmt.Sprintf("Branch '%s' is not tracked by Hitch", branchName))
		fmt.Println("\nPromote it to an environment first:")
		fmt.Printf("  hitch promote %s to dev\n", branchName)
		return fmt.Errorf("branch not tracked")
	}

	// 5. Validate branch is in at least one environment (safety check)
	if len(branchInfo.PromotedTo) == 0 {
		errorMsg(fmt.Sprintf("Branch '%s' is not in any environment", branchName))
		fmt.Println("\nSafety check: Promote to at least one environment before releasing:")
		fmt.Printf("  hitch promote %s to qa\n", branchName)
		return fmt.Errorf("branch not promoted")
	}

	// 6. Check if already merged to main
	if branchInfo.MergedToMainAt != nil {
		warning(fmt.Sprintf("%s was already merged to main on %s", branchName, branchInfo.MergedToMainAt.Format("2006-01-02")))
		fmt.Println("\nNothing to do. Use 'hitch cleanup' to remove stale branches.")
		return nil
	}

	// 7. Validate branch exists in git
	if !repo.BranchExists(branchName) {
		errorMsg(fmt.Sprintf("Branch '%s' not found in git", branchName))
		fmt.Println("\nThe branch may have been deleted. Check with:")
		fmt.Printf("  git branch -a | grep %s\n", branchName)
		return fmt.Errorf("branch not found")
	}

	// 8. Get user info
	userEmail, err := repo.UserEmail()
	if err != nil {
		errorMsg("Git user.email is not configured")
		return err
	}

	userName, _ := repo.UserName()

	baseBranch := meta.Config.BaseBranch

	fmt.Printf("Releasing %s to %s...\n\n", branchName, baseBranch)

	// Show which environments it's in
	if len(branchInfo.PromotedTo) > 0 {
		fmt.Printf("✓ Validated %s is in", branchName)
		for i, env := range branchInfo.PromotedTo {
			if i == 0 {
				fmt.Printf(" %s", env)
			} else {
				fmt.Printf(", %s", env)
			}
		}
		fmt.Println(" environment")
	}

	// 9. Checkout base branch
	if err := repo.Checkout(baseBranch); err != nil {
		errorMsg(fmt.Sprintf("Failed to checkout %s", baseBranch))
		return err
	}

	success(fmt.Sprintf("Checked out %s", baseBranch))

	// 10. Pull latest base branch
	if err := repo.Pull("origin", baseBranch); err != nil {
		warning("Failed to pull latest changes (continuing anyway)")
	}

	// 11. Merge branch into base
	mergeMsg := releaseMessage
	if mergeMsg == "" {
		mergeMsg = fmt.Sprintf("Merge %s into %s", branchName, baseBranch)
	}

	if releaseSquash {
		// Squash merge
		if err := repo.MergeSquash(branchName, mergeMsg); err != nil {
			errorMsg(fmt.Sprintf("Failed to squash merge %s into %s", branchName, baseBranch))
			fmt.Println("\nMerge conflict detected. Resolve manually:")
			fmt.Printf("  git checkout %s\n", baseBranch)
			fmt.Printf("  git merge --squash %s\n", branchName)
			fmt.Println("  # resolve conflicts")
			fmt.Println("  git commit")
			fmt.Printf("  hitch release %s\n", branchName)
			return err
		}
	} else {
		// Regular merge
		if err := repo.Merge(branchName, mergeMsg); err != nil {
			errorMsg(fmt.Sprintf("Failed to merge %s into %s", branchName, baseBranch))
			fmt.Println("\nMerge conflict detected. Resolve manually:")
			fmt.Printf("  git checkout %s\n", baseBranch)
			fmt.Printf("  git merge %s\n", branchName)
			fmt.Println("  # resolve conflicts")
			fmt.Println("  git commit")
			fmt.Printf("  hitch release %s\n", branchName)
			return err
		}
	}

	success(fmt.Sprintf("Merged %s into %s", branchName, baseBranch))

	// 12. Push base branch to remote
	if err := repo.Push("origin", baseBranch, false); err != nil {
		errorMsg(fmt.Sprintf("Failed to push %s to remote", baseBranch))
		fmt.Println("\nPush manually:")
		fmt.Printf("  git push origin %s\n", baseBranch)
		return err
	}

	success(fmt.Sprintf("Pushed %s to remote", baseBranch))

	// 13. Remove from all environments
	for _, env := range branchInfo.PromotedTo {
		if err := meta.RemoveBranchFromEnvironment(env, branchName, userEmail); err != nil {
			warning(fmt.Sprintf("Failed to remove %s from %s", branchName, env))
		}
	}

	success("Removed " + branchName + " from all environments")

	// 14. Update branch metadata - mark as merged
	now := time.Now()
	branchInfo.MergedToMainAt = &now
	branchInfo.MergedToMainBy = userEmail

	// Calculate cleanup eligibility date
	if !releaseNoDelete {
		cleanupDate := now.Add(time.Duration(meta.Config.RetentionDaysAfterMerge) * 24 * time.Hour)
		branchInfo.EligibleForCleanupAt = &cleanupDate
	}

	meta.Branches[branchName] = branchInfo

	// 15. Write metadata
	writer := metadata.NewWriter(repo.Repository)
	meta.UpdateMeta(userEmail, fmt.Sprintf("hitch release %s", branchName))
	if err := writer.Write(meta, fmt.Sprintf("Release %s to %s", branchName, baseBranch), userName, userEmail); err != nil {
		errorMsg("Failed to write metadata")
		return err
	}

	success("Updated metadata (marked merged_to_main_at)")

	fmt.Println()
	fmt.Printf("Success! %s is now in %s\n", branchName, baseBranch)

	// Show cleanup info
	if !releaseNoDelete {
		retentionDays := meta.Config.RetentionDaysAfterMerge
		if retentionDays == 1 {
			fmt.Printf("\nThe branch will be eligible for cleanup in 1 day.\n")
		} else {
			fmt.Printf("\nThe branch will be eligible for cleanup in %d days.\n", retentionDays)
		}
		fmt.Println("Use 'hitch cleanup' to delete stale branches.")
	} else {
		fmt.Println("\nBranch will not be automatically cleaned up (--no-delete specified).")
	}

	return nil
}
