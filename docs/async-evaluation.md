# Async Support Evaluation for Hitch

## Current State Analysis

### Time-Consuming Operations
1. **Branch Synchronization** (`fetch_all_remotes()`)
   - Network I/O when fetching from remotes
   - Parallelizable: Different remotes can be fetched concurrently

2. **Branch Merging** (multiple `squash_merge()` calls)
   - Each merge involves: staging changes, creating commit
   - Sequential by nature due to merge dependencies
   - Some I/O (disk writes) but mostly CPU-bound

3. **Git Operations in General**
   - Process spawning overhead
   - Disk I/O for reading/writing Git objects
   - Some CPU work (diff calculations, tree operations)

## Async Benefits Evaluation

### Where Async Would Help:
1. **Parallel Remote Operations**
   - Fetch from multiple remotes simultaneously
   - Check branch existence in parallel for initial validation

2. **Progress Reporting**
   - Show real-time progress during long operations
   - Allow cancellation with Ctrl+C

3. **Concurrent Validation**
   - Check merge conflicts for multiple branches in parallel
   - Validate branch existence across environments concurrently

### Where Async Wouldn't Help:
1. **Sequential Merge Operations**
   - Must be performed in order due to dependencies
   - Each merge changes the working directory state

2. **Single Git Operations**
   - Most git commands are already internally optimized
   - Async overhead might outweigh benefits for quick operations

## Proposed Async Architecture

### 1. Progress-Enabled Rebuild
```rust
pub async fn rebuild_environment_with_progress(
    context: &GlobalContext,
    env_name: &str,
    progress_callback: impl Fn(RebuildProgress) + Send + Sync,
) -> Result<()>
```

### 2. Parallel Operations
```rust
// Fetch all remotes concurrently
let fetch_tasks: Vec<_> = remotes
    .iter()
    .map(|remote| tokio::spawn(fetch_remote(remote)))
    .collect();

// Check merge conflicts in parallel (read-only operation)
let conflict_tasks: Vec<_> = branches
    .iter()
    .map(|branch| tokio::spawn(check_conflicts(branch)))
    .collect();
```

### 3. Interruptible Operations
```rust
pub struct RebuildContext {
    cancellation_token: CancellationToken,
    progress_sender: mpsc::Sender<RebuildProgress>,
}
```

## Implementation Considerations

### Pros:
1. **Better User Experience**
   - Real-time progress bars
   - Ability to cancel long operations
   - More responsive CLI

2. **Performance Improvements**
   - Parallel network operations
   - Concurrent validation where possible

3. **Future Extensibility**
   - Easier to add features like background operations
   - Foundation for potential GUI/TUI interfaces

### Cons:
1. **Complexity Increase**
   - More complex code with async/await
   - Need to manage cancellation tokens
   - Error handling becomes more involved

2. **Limited Benefits**
   - Git operations are largely sequential by nature
   - Most operations are already fast enough for typical use cases

3. **Dependency Overhead**
   - Tokio runtime adds binary size
   - Increased compilation time

## Recommendation

### Phase 1: Progress Reporting (High Value, Low Complexity)
Add progress reporting without full async conversion:
- Use callbacks for progress updates
- Keep sync implementation but add progress hooks
- No async runtime required

### Phase 2: Selective Async (Medium Value, Medium Complexity)
Convert only network-bound operations:
- Make `fetch_all_remotes()` async and parallel
- Keep merge operations synchronous
- Use `spawn_blocking()` for CPU-bound git operations

### Phase 3: Full Async (Low Value, High Complexity)
Convert entire pipeline to async:
- Full async/await throughout
- Cancellation support
- Parallel validation

## Alternative: Process-Level Improvements

Instead of full async, consider:
1. **Smarter Fetching**
   - Only fetch remotes that actually have relevant branches
   - Use git's built-in parallel fetch capabilities

2. **Batch Operations**
   - Combine multiple git operations where possible
   - Use git's batch mode for efficiency

3. **Caching**
   - Cache branch existence checks
   - Remember merge conflict state between runs

## Decision

Given the analysis, I recommend **Phase 1** (Progress Reporting) as it provides the most user value with minimal complexity. Full async conversion provides limited benefits for the complexity it introduces.

The rebuild process is already quite efficient, and most users won't have enough branches or complex enough histories for async to make a significant difference in performance. The main user pain point is not knowing what's happening during the process, which progress reporting solves directly.