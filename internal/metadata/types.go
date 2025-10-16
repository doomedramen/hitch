package metadata

import "time"

// Metadata represents the complete hitch.json structure
type Metadata struct {
	Version      string                 `json:"version"`
	Environments map[string]Environment `json:"environments"`
	Branches     map[string]BranchInfo  `json:"branches"`
	Config       Config                 `json:"config"`
	Meta         MetaInfo               `json:"metadata"`
}

// Environment represents a deployment environment (dev, qa, etc.)
type Environment struct {
	Base              string    `json:"base"`
	Features          []string  `json:"features"`
	Locked            bool      `json:"locked"`
	LockedBy          string    `json:"locked_by,omitempty"`
	LockedAt          time.Time `json:"locked_at,omitempty"`
	LockedReason      string    `json:"locked_reason,omitempty"`
	LastRebuild       time.Time `json:"last_rebuild,omitempty"`
	LastRebuildCommit string    `json:"last_rebuild_commit,omitempty"`
}

// BranchInfo tracks the lifecycle of a feature branch
type BranchInfo struct {
	CreatedAt           time.Time          `json:"created_at"`
	CreatedBy           string             `json:"created_by,omitempty"`
	PromotedTo          []string           `json:"promoted_to"`
	PromotedHistory     []PromotionEvent   `json:"promoted_history,omitempty"`
	MergedToMainAt      *time.Time         `json:"merged_to_main_at,omitempty"`
	MergedToMainBy      string             `json:"merged_to_main_by,omitempty"`
	LastCommitAt        time.Time          `json:"last_commit_at,omitempty"`
	LastCommitSHA       string             `json:"last_commit_sha,omitempty"`
	EligibleForCleanupAt *time.Time        `json:"eligible_for_cleanup_at,omitempty"`
}

// PromotionEvent records a single promotion/demotion event
type PromotionEvent struct {
	Environment string     `json:"environment"`
	PromotedAt  time.Time  `json:"promoted_at"`
	PromotedBy  string     `json:"promoted_by,omitempty"`
	DemotedAt   *time.Time `json:"demoted_at,omitempty"`
	DemotedBy   string     `json:"demoted_by,omitempty"`
}

// Config holds global configuration
type Config struct {
	RetentionDaysAfterMerge int      `json:"retention_days_after_merge"`
	StaleDaysNoActivity     int      `json:"stale_days_no_activity"`
	BaseBranch              string   `json:"base_branch"`
	LockTimeoutMinutes      int      `json:"lock_timeout_minutes"`
	AutoRebuildOnPromote    bool     `json:"auto_rebuild_on_promote"`
	ConflictStrategy        string   `json:"conflict_strategy"`
	NotificationWebhooks    []Webhook `json:"notification_webhooks,omitempty"`
}

// Webhook represents a notification webhook configuration
type Webhook struct {
	URL     string            `json:"url"`
	Events  []string          `json:"events"`
	Headers map[string]string `json:"headers,omitempty"`
}

// MetaInfo contains metadata about the metadata itself
type MetaInfo struct {
	InitializedAt   time.Time `json:"initialized_at"`
	InitializedBy   string    `json:"initialized_by,omitempty"`
	LastModifiedAt  time.Time `json:"last_modified_at"`
	LastModifiedBy  string    `json:"last_modified_by,omitempty"`
	LastCommand     string    `json:"last_command,omitempty"`
	HitchVersion    string    `json:"hitch_version"`
}

// NewMetadata creates a new Metadata structure with defaults
func NewMetadata(environments []string, baseBranch string, user string) *Metadata {
	envMap := make(map[string]Environment)
	for _, env := range environments {
		envMap[env] = Environment{
			Base:     baseBranch,
			Features: []string{},
			Locked:   false,
		}
	}

	now := time.Now()

	return &Metadata{
		Version:      "1.0.0",
		Environments: envMap,
		Branches:     make(map[string]BranchInfo),
		Config: Config{
			RetentionDaysAfterMerge: 7,
			StaleDaysNoActivity:     30,
			BaseBranch:              baseBranch,
			LockTimeoutMinutes:      15,
			AutoRebuildOnPromote:    true,
			ConflictStrategy:        "abort",
			NotificationWebhooks:    []Webhook{},
		},
		Meta: MetaInfo{
			InitializedAt:  now,
			InitializedBy:  user,
			LastModifiedAt: now,
			LastModifiedBy: user,
			LastCommand:    "hitch init",
			HitchVersion:   "1.0.0", // TODO: Get from build
		},
	}
}

// UpdateMeta updates the metadata modification tracking
func (m *Metadata) UpdateMeta(user, command string) {
	m.Meta.LastModifiedAt = time.Now()
	m.Meta.LastModifiedBy = user
	m.Meta.LastCommand = command
}

// IsEnvironmentLocked checks if an environment is locked
func (m *Metadata) IsEnvironmentLocked(env string) bool {
	e, exists := m.Environments[env]
	if !exists {
		return false
	}
	return e.Locked
}

// IsLockedByUser checks if an environment is locked by a specific user
func (m *Metadata) IsLockedByUser(env string, user string) bool {
	e, exists := m.Environments[env]
	if !exists {
		return false
	}
	return e.Locked && e.LockedBy == user
}

// IsLockStale checks if a lock is older than the timeout
func (m *Metadata) IsLockStale(env string) bool {
	e, exists := m.Environments[env]
	if !exists || !e.Locked {
		return false
	}

	lockAge := time.Since(e.LockedAt)
	timeout := time.Duration(m.Config.LockTimeoutMinutes) * time.Minute
	return lockAge > timeout
}

// LockEnvironment locks an environment
func (m *Metadata) LockEnvironment(env string, user string, reason string) error {
	e, exists := m.Environments[env]
	if !exists {
		return &EnvironmentNotFoundError{Environment: env}
	}

	if e.Locked && !m.IsLockStale(env) && e.LockedBy != user {
		return &EnvironmentLockedError{
			Environment: env,
			LockedBy:    e.LockedBy,
			LockedAt:    e.LockedAt,
		}
	}

	e.Locked = true
	e.LockedBy = user
	e.LockedAt = time.Now()
	e.LockedReason = reason

	m.Environments[env] = e
	return nil
}

// UnlockEnvironment unlocks an environment
func (m *Metadata) UnlockEnvironment(env string) error {
	e, exists := m.Environments[env]
	if !exists {
		return &EnvironmentNotFoundError{Environment: env}
	}

	e.Locked = false
	e.LockedBy = ""
	e.LockedReason = ""

	m.Environments[env] = e
	return nil
}

// AddBranchToEnvironment adds a branch to an environment's feature list
func (m *Metadata) AddBranchToEnvironment(env string, branch string, user string) error {
	e, exists := m.Environments[env]
	if !exists {
		return &EnvironmentNotFoundError{Environment: env}
	}

	// Check if already present
	for _, f := range e.Features {
		if f == branch {
			return nil // Already present
		}
	}

	e.Features = append(e.Features, branch)
	m.Environments[env] = e

	// Update branch info
	info, exists := m.Branches[branch]
	if !exists {
		info = BranchInfo{
			CreatedAt:       time.Now(),
			CreatedBy:       user,
			PromotedTo:      []string{},
			PromotedHistory: []PromotionEvent{},
		}
	}

	// Add to promoted_to if not already there
	found := false
	for _, e := range info.PromotedTo {
		if e == env {
			found = true
			break
		}
	}
	if !found {
		info.PromotedTo = append(info.PromotedTo, env)
	}

	// Add promotion event
	info.PromotedHistory = append(info.PromotedHistory, PromotionEvent{
		Environment: env,
		PromotedAt:  time.Now(),
		PromotedBy:  user,
	})

	m.Branches[branch] = info
	return nil
}

// RemoveBranchFromEnvironment removes a branch from an environment's feature list
func (m *Metadata) RemoveBranchFromEnvironment(env string, branch string, user string) error {
	e, exists := m.Environments[env]
	if !exists {
		return &EnvironmentNotFoundError{Environment: env}
	}

	// Remove from features list
	newFeatures := []string{}
	for _, f := range e.Features {
		if f != branch {
			newFeatures = append(newFeatures, f)
		}
	}
	e.Features = newFeatures
	m.Environments[env] = e

	// Update branch info
	info, exists := m.Branches[branch]
	if exists {
		// Remove from promoted_to
		newPromotedTo := []string{}
		for _, e := range info.PromotedTo {
			if e != env {
				newPromotedTo = append(newPromotedTo, e)
			}
		}
		info.PromotedTo = newPromotedTo

		// Mark demotion in history
		for i := len(info.PromotedHistory) - 1; i >= 0; i-- {
			if info.PromotedHistory[i].Environment == env && info.PromotedHistory[i].DemotedAt == nil {
				now := time.Now()
				info.PromotedHistory[i].DemotedAt = &now
				info.PromotedHistory[i].DemotedBy = user
				break
			}
		}

		m.Branches[branch] = info
	}

	return nil
}
