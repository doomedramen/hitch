package cmd

import (
	"fmt"
	"os/exec"
	"strings"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/spf13/cobra"
)

var (
	initEnvironments string
	initBaseBranch   string
	initRetentionDays int
	initStaleDays    int
)

var initCmd = &cobra.Command{
	Use:   "init",
	Short: "Initialize Hitch in the current Git repository",
	Long: `Initialize Hitch in the current Git repository.

This command:
1. Verifies the current directory is a Git repository
2. Creates a 'hitch-metadata' orphan branch
3. Writes initial configuration to hitch.json
4. Pushes the metadata branch to remote

After initialization, you can start promoting features to environments.`,
	RunE: runInit,
}

func init() {
	initCmd.Flags().StringVar(&initEnvironments, "environments", "dev,qa", "Comma-separated list of environments")
	initCmd.Flags().StringVar(&initBaseBranch, "base", "main", "Base branch name")
	initCmd.Flags().IntVar(&initRetentionDays, "retention-days", 7, "Days to keep branches after merge")
	initCmd.Flags().IntVar(&initStaleDays, "stale-days", 30, "Days before warning about inactive branches")
}

func runInit(cmd *cobra.Command, args []string) error {
	// 1. Open Git repository
	repo, err := hitchgit.OpenRepo(".")
	if err != nil {
		errorMsg("Not a Git repository")
		fmt.Println("\nPlease run 'hitch init' from within a Git repository.")
		fmt.Println("To create a Git repository, run: git init")
		return err
	}

	// 2. Check if already initialized
	reader := metadata.NewReader(repo.Repository)
	if reader.Exists() {
		warning("Hitch is already initialized in this repository")
		fmt.Println("\nTo reinitialize, first delete the hitch-metadata branch:")
		fmt.Println("  git branch -D hitch-metadata")
		fmt.Println("  git push origin --delete hitch-metadata")
		return fmt.Errorf("hitch already initialized")
	}

	// 3. Get user info
	userName, err := repo.UserName()
	if err != nil {
		warning("Could not get git user.name, using default")
		userName = "Unknown"
	}

	userEmail, err := repo.UserEmail()
	if err != nil {
		errorMsg("Git user.email is not configured")
		fmt.Println("\nPlease configure your Git user email:")
		fmt.Println("  git config --global user.email \"you@example.com\"")
		return err
	}

	// 4. Parse environments
	envList := strings.Split(initEnvironments, ",")
	for i, env := range envList {
		envList[i] = strings.TrimSpace(env)
	}

	info(fmt.Sprintf("Initializing Hitch with environments: %s", strings.Join(envList, ", ")))

	// 5. Create metadata
	meta := metadata.NewMetadata(envList, initBaseBranch, userEmail)
	meta.Config.RetentionDaysAfterMerge = initRetentionDays
	meta.Config.StaleDaysNoActivity = initStaleDays

	// 6. Create hitch-metadata orphan branch using git command
	// Note: go-git doesn't handle orphan branches well, so we use exec
	if err := createOrphanBranch(repo, userName, userEmail, meta); err != nil {
		errorMsg("Failed to create hitch-metadata branch")
		return err
	}

	success("Hitch initialized successfully")
	fmt.Println()
	fmt.Println("Environments configured:", strings.Join(envList, ", "))
	fmt.Println("Base branch:", initBaseBranch)
	fmt.Println()
	fmt.Println("Next steps:")
	fmt.Println("  1. Create a feature branch: git checkout -b feature/my-feature")
	fmt.Println("  2. Promote to dev: hitch promote feature/my-feature to dev")
	fmt.Println("  3. Check status: hitch status")

	return nil
}

// createOrphanBranch creates the hitch-metadata orphan branch using git commands
func createOrphanBranch(repo *hitchgit.Repo, userName, userEmail string, meta *metadata.Metadata) error {
	// Remember current branch
	currentBranch, err := repo.CurrentBranch()
	if err != nil {
		// Maybe detached HEAD, get commit
		currentCommit, err := repo.CurrentCommitSHA()
		if err != nil {
			return fmt.Errorf("failed to get current state: %w", err)
		}
		currentBranch = currentCommit
	}

	// Create orphan branch
	cmd := exec.Command("git", "checkout", "--orphan", metadata.MetadataBranch)
	if output, err := cmd.CombinedOutput(); err != nil {
		return fmt.Errorf("failed to create orphan branch: %s", string(output))
	}

	// Remove all files from index
	cmd = exec.Command("git", "rm", "-rf", "--cached", ".")
	cmd.Run() // Ignore error, there might be no files

	// Write hitch.json using metadata writer
	writer := metadata.NewWriter(repo.Repository)
	if err := writer.WriteInitial(meta, userName, userEmail); err != nil {
		// Cleanup: return to original branch
		exec.Command("git", "checkout", currentBranch).Run()
		exec.Command("git", "branch", "-D", metadata.MetadataBranch).Run()
		return fmt.Errorf("failed to write initial metadata: %w", err)
	}

	// Push to remote
	cmd = exec.Command("git", "push", "-u", "origin", metadata.MetadataBranch)
	if output, err := cmd.CombinedOutput(); err != nil {
		warning("Failed to push hitch-metadata branch to remote")
		fmt.Println("You may need to push manually:")
		fmt.Printf("  git push -u origin %s\n", metadata.MetadataBranch)
		fmt.Println()
		fmt.Println("Error:", string(output))
		// Don't fail, local init succeeded
	}

	// Return to original branch
	if err := repo.Checkout(currentBranch); err != nil {
		warning(fmt.Sprintf("Failed to return to original branch %s", currentBranch))
		fmt.Println("You may need to checkout manually:")
		fmt.Printf("  git checkout %s\n", currentBranch)
	}

	return nil
}
