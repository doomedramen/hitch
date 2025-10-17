package cmd

import (
	"fmt"
	"os"

	"github.com/fatih/color"
	"github.com/spf13/cobra"
)

const version = "1.0.0"

var (
	verbose bool
	noColor bool
)

// rootCmd represents the base command
var rootCmd = &cobra.Command{
	Use:     "hitch",
	Short:   "Git workflow manager for multi-environment development",
	Version: version,
	PersistentPreRun: func(cmd *cobra.Command, args []string) {
		if noColor {
			color.NoColor = true
		}
	},
}

// Execute runs the root command
func Execute() error {
	return rootCmd.Execute()
}

func init() {
	rootCmd.PersistentFlags().BoolVarP(&verbose, "verbose", "v", false, "Enable verbose output")
	rootCmd.PersistentFlags().BoolVar(&noColor, "no-color", false, "Disable colored output")

	// Add subcommands
	rootCmd.AddCommand(initCmd)
	rootCmd.AddCommand(statusCmd)
	rootCmd.AddCommand(hookCmd)
}

// Helper functions for colored output

func success(msg string) {
	fmt.Fprintf(os.Stdout, "%s %s\n", color.GreenString("✓"), msg)
}

func warning(msg string) {
	fmt.Fprintf(os.Stderr, "%s %s\n", color.YellowString("⚠"), msg)
}

func errorMsg(msg string) {
	fmt.Fprintf(os.Stderr, "%s %s\n", color.RedString("❌"), msg)
}

func info(msg string) {
	fmt.Fprintf(os.Stdout, "%s\n", msg)
}
