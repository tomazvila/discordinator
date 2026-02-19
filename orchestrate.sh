#!/bin/bash

# Orchestrator: Parallel Ralph Workers via Git Worktrees
#
# Runs the Discordinator Ralph loop across multiple git worktrees in phases.
# Phase 1 builds the foundation sequentially, then fans out to parallel workers.
#
# Usage:
#   ./orchestrate.sh                    # Full 5-phase run
#   ./orchestrate.sh --phase 2          # Start from phase 2 (foundation already merged)
#   ./orchestrate.sh --max-iter 30      # Limit iterations per worker
#   ./orchestrate.sh --dry-run          # Show plan without executing

set -euo pipefail

# Ensure tools are in PATH
export PATH="$HOME/.local/bin:$HOME/.nix-profile/bin:/nix/var/nix/profiles/default/bin:/opt/homebrew/bin:$PATH"

# ============================================================================
# Configuration
# ============================================================================

MAIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKTREE_BASE="$(dirname "$MAIN_DIR")"
MAX_ITER_PER_WORKER=50
START_PHASE=1
DRY_RUN=false
POLL_INTERVAL=30  # seconds between status checks

# Task assignments per phase
# Phase 1: Foundation (sequential) — everything else depends on this
PHASE1_TASKS="1,2,3,4,5"

# Phase 2: Three parallel workers
PHASE2_WORKER_B_TASKS="6,7,8,9,11"        # Infrastructure (gateway, HTTP, auth)
PHASE2_WORKER_C_TASKS="12,13,14,15,16,17,18,19,20"  # UI foundation
PHASE2_WORKER_D_TASKS="28,29,30,31"        # Markdown + pane data structure

# Phase 4: Two parallel workers (after phase 2+3 merge)
PHASE4_WORKER_E_TASKS="21,22,23,24,25,26,27"  # Core features
PHASE4_WORKER_F_TASKS="32,33,34,35,36,37,10"  # Pane system + cache (10 needs types done)

# Phase 4 continued: Login (depends on auth from worker B)
PHASE4_WORKER_G_TASKS="38,39,40"           # Login UI

# ============================================================================
# Argument Parsing
# ============================================================================

while [[ $# -gt 0 ]]; do
    case "$1" in
        --phase)
            START_PHASE="$2"
            shift 2
            ;;
        --max-iter)
            MAX_ITER_PER_WORKER="$2"
            shift 2
            ;;
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        --poll)
            POLL_INTERVAL="$2"
            shift 2
            ;;
        -h|--help)
            echo "Usage: ./orchestrate.sh [options]"
            echo ""
            echo "Options:"
            echo "  --phase N       Start from phase N (1-5, default: 1)"
            echo "  --max-iter N    Max iterations per worker (default: 50)"
            echo "  --dry-run       Show plan without executing"
            echo "  --poll N        Status poll interval in seconds (default: 30)"
            echo ""
            echo "Phases:"
            echo "  1: Foundation (sequential) — Tasks $PHASE1_TASKS"
            echo "  2: Parallel workers B,C,D — Tasks $PHASE2_WORKER_B_TASKS | $PHASE2_WORKER_C_TASKS | $PHASE2_WORKER_D_TASKS"
            echo "  3: Merge phase 2 into main"
            echo "  4: Parallel workers E,F,G — Tasks $PHASE4_WORKER_E_TASKS | $PHASE4_WORKER_F_TASKS | $PHASE4_WORKER_G_TASKS"
            echo "  5: Final merge + verification"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# ============================================================================
# Helper Functions
# ============================================================================

log() {
    echo "[$(date '+%H:%M:%S')] $*"
}

log_phase() {
    echo ""
    echo "================================================================"
    echo "[$(date '+%H:%M:%S')] PHASE $1: $2"
    echo "================================================================"
    echo ""
}

die() {
    echo "FATAL: $*" >&2
    exit 1
}

worktree_dir() {
    local name="$1"
    echo "${WORKTREE_BASE}/discordinator-${name}"
}

# Create a worktree branching from main
create_worktree() {
    local name="$1"
    local branch="worker-${name}"
    local dir
    dir="$(worktree_dir "$name")"

    if [ -d "$dir" ]; then
        log "Worktree $name already exists at $dir, reusing"
        return 0
    fi

    log "Creating worktree: $dir (branch: $branch)"
    git -C "$MAIN_DIR" worktree add "$dir" -b "$branch" main
}

# Remove a worktree
remove_worktree() {
    local name="$1"
    local dir
    dir="$(worktree_dir "$name")"

    if [ -d "$dir" ]; then
        log "Removing worktree: $dir"
        git -C "$MAIN_DIR" worktree remove "$dir" --force 2>/dev/null || true
    fi

    # Clean up branch
    local branch="worker-${name}"
    git -C "$MAIN_DIR" branch -D "$branch" 2>/dev/null || true
}

# Launch ralph.sh in a worktree as a background process
launch_worker() {
    local name="$1"
    local tasks="$2"
    local dir
    dir="$(worktree_dir "$name")"
    local log_file="${dir}/ralph_log_${name}.md"
    local pid_file="/tmp/ralph_pid_${name}.txt"
    local worker_log="/tmp/ralph_worker_${name}.log"

    log "Launching worker '$name' in $dir"
    log "  Tasks: $tasks"
    log "  Max iterations: $MAX_ITER_PER_WORKER"
    log "  Output: $worker_log"

    (
        cd "$dir"
        bash ./ralph.sh --tasks "$tasks" --name "$name" "$MAX_ITER_PER_WORKER" \
            > "$worker_log" 2>&1
    ) &

    local pid=$!
    echo "$pid" > "$pid_file"
    log "  PID: $pid"
}

# Check if a worker is still running
worker_running() {
    local name="$1"
    local pid_file="/tmp/ralph_pid_${name}.txt"

    if [ ! -f "$pid_file" ]; then
        return 1
    fi

    local pid
    pid=$(cat "$pid_file")
    kill -0 "$pid" 2>/dev/null
}

# Get worker exit code (only valid after worker exits)
worker_exit_code() {
    local name="$1"
    local pid_file="/tmp/ralph_pid_${name}.txt"

    if [ ! -f "$pid_file" ]; then
        echo "unknown"
        return
    fi

    local pid
    pid=$(cat "$pid_file")
    wait "$pid" 2>/dev/null
    echo $?
}

# Get worker status from JSON status file
worker_status() {
    local name="$1"
    local dir
    dir="$(worktree_dir "$name")"
    local status_file="${dir}/.ralph_status_${name}.json"

    if [ -f "$status_file" ]; then
        jq -r '.status' "$status_file" 2>/dev/null || echo "unknown"
    else
        echo "unknown"
    fi
}

# Get worker iteration from JSON status file
worker_iteration() {
    local name="$1"
    local dir
    dir="$(worktree_dir "$name")"
    local status_file="${dir}/.ralph_status_${name}.json"

    if [ -f "$status_file" ]; then
        jq -r '.iteration' "$status_file" 2>/dev/null || echo "0"
    else
        echo "0"
    fi
}

# Wait for all workers in a list to finish, showing progress
wait_for_workers() {
    local workers=("$@")
    local all_done=false

    log "Waiting for workers: ${workers[*]}"
    echo ""

    while ! $all_done; do
        all_done=true
        local status_line=""

        for name in "${workers[@]}"; do
            if worker_running "$name"; then
                all_done=false
                local iter
                iter=$(worker_iteration "$name")
                status_line+="  $name: running (iter $iter/$MAX_ITER_PER_WORKER)"
            else
                local status
                status=$(worker_status "$name")
                if [ "$status" = "complete" ]; then
                    status_line+="  $name: DONE"
                else
                    status_line+="  $name: STOPPED ($status)"
                fi
            fi
            status_line+=$'\n'
        done

        printf "\r\033[K"
        echo "--- Worker Status [$(date '+%H:%M:%S')] ---"
        echo "$status_line"

        if ! $all_done; then
            sleep "$POLL_INTERVAL"
            # Move cursor up to overwrite status block
            local lines
            lines=$(echo "$status_line" | wc -l)
            printf "\033[%dA\033[K" "$((lines + 1))"
        fi
    done

    echo ""
    log "All workers finished"
}

# Merge a worker branch into main with conflict resolution
merge_worker() {
    local name="$1"
    local branch="worker-${name}"

    log "Merging branch '$branch' into main..."

    cd "$MAIN_DIR"
    git checkout main

    if git merge "$branch" --no-edit 2>/dev/null; then
        log "  Merged '$branch' cleanly"
        return 0
    fi

    log "  Merge conflicts detected, resolving..."

    # Auto-resolve Cargo.lock — always regenerate
    if git diff --name-only --diff-filter=U | grep -q "Cargo.lock"; then
        log "  Resolving Cargo.lock (will regenerate)"
        git checkout --ours Cargo.lock 2>/dev/null || true
        git add Cargo.lock
    fi

    # Auto-resolve mod.rs files — union merge (both sides are additive pub mod lines)
    for modfile in $(git diff --name-only --diff-filter=U | grep "mod\.rs$"); do
        log "  Resolving $modfile (union merge)"
        # Accept both sides by combining unique lines
        if git show :2:"$modfile" > /tmp/merge_ours_$$.rs 2>/dev/null && \
           git show :3:"$modfile" > /tmp/merge_theirs_$$.rs 2>/dev/null; then
            # Combine, sort, deduplicate (works for mod.rs files with pub mod lines)
            sort -u /tmp/merge_ours_$$.rs /tmp/merge_theirs_$$.rs > "$modfile"
            git add "$modfile"
        fi
        rm -f /tmp/merge_ours_$$.rs /tmp/merge_theirs_$$.rs
    done

    # Check for remaining conflicts
    local remaining
    remaining=$(git diff --name-only --diff-filter=U 2>/dev/null || true)
    if [ -n "$remaining" ]; then
        log "  WARNING: Unresolved conflicts in:"
        echo "$remaining" | sed 's/^/    /'
        log "  Attempting to accept theirs for remaining files..."
        for f in $remaining; do
            git checkout --theirs "$f" 2>/dev/null || true
            git add "$f"
        done
    fi

    git commit --no-edit 2>/dev/null || git commit -m "Merge $branch into main (auto-resolved conflicts)"

    # Regenerate Cargo.lock
    log "  Regenerating Cargo.lock..."
    if nix develop --command cargo check 2>/dev/null; then
        git add Cargo.lock 2>/dev/null || true
        git commit -m "Regenerate Cargo.lock after merging $branch" 2>/dev/null || true
        log "  Cargo.lock regenerated"
    else
        log "  WARNING: cargo check failed after merge — manual fix may be needed"
    fi

    log "  Merge of '$branch' complete"
}

# Full build + test + clippy verification
verify_build() {
    log "Running full verification..."

    cd "$MAIN_DIR"
    local passed=true

    log "  cargo build..."
    if ! nix develop --command cargo build 2>&1; then
        log "  BUILD FAILED"
        passed=false
    fi

    log "  cargo test..."
    if ! nix develop --command cargo test 2>&1; then
        log "  TESTS FAILED"
        passed=false
    fi

    log "  cargo clippy..."
    if ! nix develop --command cargo clippy -- -D warnings 2>&1; then
        log "  CLIPPY FAILED"
        passed=false
    fi

    if $passed; then
        log "  Verification PASSED"
        return 0
    else
        log "  Verification FAILED"
        return 1
    fi
}

# ============================================================================
# Dry Run
# ============================================================================

if $DRY_RUN; then
    echo "=== ORCHESTRATION PLAN (dry run) ==="
    echo ""
    echo "Main repo: $MAIN_DIR"
    echo "Worktree base: $WORKTREE_BASE"
    echo "Max iterations per worker: $MAX_ITER_PER_WORKER"
    echo "Starting from phase: $START_PHASE"
    echo ""
    echo "Phase 1 — Foundation (sequential)"
    echo "  Worker: foundation"
    echo "  Tasks: $PHASE1_TASKS"
    echo "  Dir: $(worktree_dir foundation)"
    echo ""
    echo "Phase 2 — Parallel workers (3)"
    echo "  Worker B: infra       Tasks: $PHASE2_WORKER_B_TASKS   Dir: $(worktree_dir infra)"
    echo "  Worker C: ui          Tasks: $PHASE2_WORKER_C_TASKS   Dir: $(worktree_dir ui)"
    echo "  Worker D: markdown    Tasks: $PHASE2_WORKER_D_TASKS   Dir: $(worktree_dir markdown)"
    echo ""
    echo "Phase 3 — Sequential merge: infra -> ui -> markdown -> main"
    echo ""
    echo "Phase 4 — Parallel workers (3)"
    echo "  Worker E: features    Tasks: $PHASE4_WORKER_E_TASKS   Dir: $(worktree_dir features)"
    echo "  Worker F: panes       Tasks: $PHASE4_WORKER_F_TASKS   Dir: $(worktree_dir panes)"
    echo "  Worker G: login       Tasks: $PHASE4_WORKER_G_TASKS   Dir: $(worktree_dir login)"
    echo ""
    echo "Phase 5 — Final merge + verification"
    echo ""
    echo "Total estimated disk: ~15-25GB (target/ dirs across worktrees)"
    echo "Total estimated token cost: ~15x single-worker run"
    echo ""
    echo "=== END DRY RUN ==="
    exit 0
fi

# ============================================================================
# Preflight Checks
# ============================================================================

log "Preflight checks..."

command -v git >/dev/null || die "git not found"
command -v claude >/dev/null || die "claude not found"
command -v nix >/dev/null || die "nix not found"
command -v jq >/dev/null || die "jq not found"

# Must be on main branch
cd "$MAIN_DIR"
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "main" ]; then
    die "Must be on 'main' branch (currently on '$CURRENT_BRANCH')"
fi

# Must have clean working directory
if [ -n "$(git status --porcelain)" ]; then
    die "Working directory is not clean. Commit or stash changes first."
fi

log "All checks passed"

# Record start time
ORCHESTRATE_START=$(date +%s)

# ============================================================================
# Phase 1: Foundation (Sequential)
# ============================================================================

if [ "$START_PHASE" -le 1 ]; then
    log_phase 1 "Foundation (sequential) — Tasks $PHASE1_TASKS"

    create_worktree "foundation"
    launch_worker "foundation" "$PHASE1_TASKS"
    wait_for_workers "foundation"

    # Check result
    if [ "$(worker_status "foundation")" != "complete" ]; then
        log "WARNING: Foundation worker did not signal completion"
        log "Check logs at /tmp/ralph_worker_foundation.log"
        log "Attempting merge anyway..."
    fi

    merge_worker "foundation"
    remove_worktree "foundation"

    log "Phase 1 complete — foundation merged into main"
fi

# ============================================================================
# Phase 2: Parallel Workers B, C, D
# ============================================================================

if [ "$START_PHASE" -le 2 ]; then
    log_phase 2 "Parallel workers (3) — Infrastructure, UI, Markdown+Panes"

    create_worktree "infra"
    create_worktree "ui"
    create_worktree "markdown"

    launch_worker "infra" "$PHASE2_WORKER_B_TASKS"
    launch_worker "ui" "$PHASE2_WORKER_C_TASKS"
    launch_worker "markdown" "$PHASE2_WORKER_D_TASKS"

    wait_for_workers "infra" "ui" "markdown"

    log "Phase 2 complete — all parallel workers finished"
fi

# ============================================================================
# Phase 3: Merge Phase 2 Results
# ============================================================================

if [ "$START_PHASE" -le 3 ]; then
    log_phase 3 "Merging phase 2 workers into main"

    # Merge in dependency order: infra first (other code may reference its types)
    merge_worker "infra"
    merge_worker "ui"
    merge_worker "markdown"

    # Verify the merge builds
    log "Verifying post-merge build..."
    if verify_build; then
        log "Phase 3 merge verified"
    else
        log "WARNING: Post-merge verification failed"
        log "Continuing anyway — phase 4 workers may fix issues"
    fi

    # Cleanup phase 2 worktrees
    remove_worktree "infra"
    remove_worktree "ui"
    remove_worktree "markdown"

    log "Phase 3 complete"
fi

# ============================================================================
# Phase 4: Parallel Workers E, F, G
# ============================================================================

if [ "$START_PHASE" -le 4 ]; then
    log_phase 4 "Parallel workers (3) — Features, Panes, Login"

    create_worktree "features"
    create_worktree "panes"
    create_worktree "login"

    launch_worker "features" "$PHASE4_WORKER_E_TASKS"
    launch_worker "panes" "$PHASE4_WORKER_F_TASKS"
    launch_worker "login" "$PHASE4_WORKER_G_TASKS"

    wait_for_workers "features" "panes" "login"

    log "Phase 4 complete — all parallel workers finished"
fi

# ============================================================================
# Phase 5: Final Merge + Verification
# ============================================================================

if [ "$START_PHASE" -le 5 ]; then
    log_phase 5 "Final merge + verification"

    merge_worker "features"
    merge_worker "panes"
    merge_worker "login"

    # Cleanup phase 4 worktrees
    remove_worktree "features"
    remove_worktree "panes"
    remove_worktree "login"

    log "Running final verification..."
    if verify_build; then
        ORCHESTRATE_END=$(date +%s)
        ELAPSED=$(( ORCHESTRATE_END - ORCHESTRATE_START ))
        HOURS=$(( ELAPSED / 3600 ))
        MINUTES=$(( (ELAPSED % 3600) / 60 ))

        echo ""
        echo "================================================================"
        echo "=== ORCHESTRATION COMPLETE ==="
        echo "=== Total time: ${HOURS}h ${MINUTES}m ==="
        echo "================================================================"
        exit 0
    else
        echo ""
        echo "================================================================"
        echo "=== ORCHESTRATION FINISHED WITH FAILURES ==="
        echo "=== Run 'nix develop --command cargo test' to see issues ==="
        echo "================================================================"
        exit 1
    fi
fi
