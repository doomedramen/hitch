package cmd

import (
	"fmt"
	"time"

	hitchgit "github.com/DoomedRamen/hitch/internal/git"
	"github.com/DoomedRamen/hitch/internal/metadata"
	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

var (
	statusStale bool
	statusEnv   string
	statusJSON  bool
)

var statusCmd = &cobra.Command{
	Use:   "status",
	Short: "Show current state of all environments and branches",
	Long: `Show current state of all environments and branches.

Displays:
- Which features are in each environment
- Lock status
- Optionally, stale branches`,
	RunE: runStatus,
}

func init() {
	statusCmd.Flags().BoolVar(&statusStale, "stale", false, "Include stale branch analysis")
	statusCmd.Flags().StringVar(&statusEnv, "env", "", "Show only specific environment")
	statusCmd.Flags().BoolVar(&statusJSON, "json", false, "Output as JSON")
}

func runStatus(cmd *cobra.Command, args []string) error {
	// 1. Open Git repository
	repo, err := hitchgit.OpenRepo(".")
	if err != nil {
		errorMsg("Not a Git repository")
		return err
	}

	// 2. Read metadata
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

	// 3. Display status
	if statusJSON {
		return displayJSONStatus(meta)
	}

	return displayHumanStatus(meta)
}

func displayHumanStatus(meta *metadata.Metadata) error {
	color.New(color.Bold).Println("Hitch Status")
	fmt.Println()

	// Display each environment
	for envName, env := range meta.Environments {
		// Skip if filtering by specific environment
		if statusEnv != "" && envName != statusEnv {
			continue
		}

		// Environment header
		lockStatus := color.GreenString("unlocked")
		if env.Locked {
			lockStatus = color.RedString("locked by %s since %s", env.LockedBy, env.LockedAt.Format("15:04:05"))
			if meta.IsLockStale(envName) {
				lockStatus += color.YellowString(" (STALE)")
			}
		}

		fmt.Printf("Environment: %s (%s)\n", color.CyanString(envName), lockStatus)
		fmt.Printf("  Base: %s\n", env.Base)

		if len(env.Features) == 0 {
			fmt.Println("  Features: (none)")
		} else {
			fmt.Println("  Features:")
			for _, feature := range env.Features {
				// Get promotion time if available
				branchInfo, exists := meta.Branches[feature]
				timeStr := ""
				if exists {
					for _, event := range branchInfo.PromotedHistory {
						if event.Environment == envName && event.DemotedAt == nil {
							timeStr = fmt.Sprintf(" (promoted %s)", formatTimeAgo(event.PromotedAt))
							break
						}
					}
				}
				fmt.Printf("    - %s%s\n", feature, timeStr)
			}
		}

		if !env.LastRebuild.IsZero() {
			fmt.Printf("  Last rebuild: %s\n", formatTimeAgo(env.LastRebuild))
		}

		fmt.Println()
	}

	// Display stale branches if requested
	if statusStale {
		displayStaleBranches(meta)
	}

	return nil
}

func displayStaleBranches(meta *metadata.Metadata) {
	safeTodelete := []string{}
	inactive := []string{}

	for branchName, info := range meta.Branches {
		// Safe to delete if merged + past retention period + not in any env
		if info.MergedToMainAt != nil {
			daysSinceMerge := int(time.Since(*info.MergedToMainAt).Hours() / 24)
			if daysSinceMerge > meta.Config.RetentionDaysAfterMerge {
				// Check not in any environment
				inEnv := false
				for _, env := range meta.Environments {
					for _, f := range env.Features {
						if f == branchName {
							inEnv = true
							break
						}
					}
					if inEnv {
						break
					}
				}
				if !inEnv {
					safeTodelete = append(safeTodelete, fmt.Sprintf("%s (merged %d days ago)", branchName, daysSinceMerge))
				}
			}
		}

		// Warn about inactive branches
		if info.MergedToMainAt == nil && !info.LastCommitAt.IsZero() {
			daysSinceCommit := int(time.Since(info.LastCommitAt).Hours() / 24)
			if daysSinceCommit > meta.Config.StaleDaysNoActivity {
				inactive = append(inactive, fmt.Sprintf("%s (last commit %d days ago)", branchName, daysSinceCommit))
			}
		}
	}

	if len(safeTodelete) > 0 || len(inactive) > 0 {
		color.New(color.Bold).Println("Stale Branches")
		fmt.Println()

		if len(safeTodelete) > 0 {
			fmt.Println("Safe to delete (merged to main):")
			for _, branch := range safeTodelete {
				fmt.Printf("  ✓ %s\n", branch)
			}
			fmt.Println()
		}

		if len(inactive) > 0 {
			fmt.Println("Inactive branches (no recent commits):")
			for _, branch := range inactive {
				fmt.Printf("  ? %s\n", branch)
			}
			fmt.Println()
		}

		fmt.Println("Run 'hitch cleanup' to delete stale branches")
	}
}

func displayJSONStatus(meta *metadata.Metadata) error {
	// TODO: Implement JSON output
	return fmt.Errorf("JSON output not yet implemented")
}

func formatTimeAgo(t time.Time) string {
	duration := time.Since(t)

	if duration < time.Minute {
		return "just now"
	}

	if duration < time.Hour {
		minutes := int(duration.Minutes())
		if minutes == 1 {
			return "1 minute ago"
		}
		return fmt.Sprintf("%d minutes ago", minutes)
	}

	if duration < 24*time.Hour {
		hours := int(duration.Hours())
		if hours == 1 {
			return "1 hour ago"
		}
		return fmt.Sprintf("%d hours ago", hours)
	}

	days := int(duration.Hours() / 24)
	if days == 1 {
		return "1 day ago"
	}
	return fmt.Sprintf("%d days ago", days)
}
