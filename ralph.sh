#!/bin/bash

# Ralph Wiggum Loop for Discordinator Development
# Reads REQUIREMENTS.md and works through tasks until assigned tasks pass
#
# Usage:
#   ./ralph.sh [max_iterations]                    # All tasks (standalone mode)
#   ./ralph.sh --tasks "1,2,3,4,5" [max_iter]     # Specific tasks (worker mode)
#   ./ralph.sh --tasks "6,7,8,9,11" --name infra  # Named worker

set -euo pipefail

# Ensure claude and nix are in PATH
export PATH="$HOME/.local/bin:$HOME/.nix-profile/bin:/nix/var/nix/profiles/default/bin:/opt/homebrew/bin:$PATH"

# --- Argument Parsing ---

TASK_LIST=""
WORKER_NAME=""
MAX_ITERATIONS=50

while [[ $# -gt 0 ]]; do
    case "$1" in
        --tasks)
            TASK_LIST="$2"
            shift 2
            ;;
        --name)
            WORKER_NAME="$2"
            shift 2
            ;;
        *)
            MAX_ITERATIONS="$1"
            shift
            ;;
    esac
done

# --- Helper Functions ---

run_claude_streaming() {
    local prompt="$1"
    local output_file="$2"
    local raw_file="/tmp/ralph_raw_$$.json"

    > "$output_file"

    if ! command -v jq &> /dev/null; then
        echo "ERROR: jq is required. It should be available via nix develop."
        exit 1
    fi

    claude --print --output-format stream-json --verbose --dangerously-skip-permissions -p "$prompt" 2>&1 | \
    tee "$raw_file" | \
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        msg_type=$(echo "$line" | jq -r '.type // empty' 2>/dev/null)
        case "$msg_type" in
            "assistant")
                content=$(echo "$line" | jq -r '.message.content[]? | select(.type=="text") | .text // empty' 2>/dev/null)
                if [ -n "$content" ]; then
                    printf "%s" "$content"
                    printf "%s" "$content" >> "$output_file"
                fi
                ;;
            "content_block_start")
                tool_name=$(echo "$line" | jq -r '.content_block.name // empty' 2>/dev/null)
                if [ -n "$tool_name" ]; then
                    printf "\n>>> [Tool: %s]\n" "$tool_name"
                    printf "\n>>> [Tool: %s]\n" "$tool_name" >> "$output_file"
                fi
                ;;
            "content_block_delta")
                delta=$(echo "$line" | jq -r '.delta.text // empty' 2>/dev/null)
                if [ -n "$delta" ]; then
                    printf "%s" "$delta"
                    printf "%s" "$delta" >> "$output_file"
                fi
                ;;
            "result")
                content=$(echo "$line" | jq -r '.result // empty' 2>/dev/null)
                if [ -n "$content" ] && [ "$content" != "null" ]; then
                    printf "%s\n" "$content"
                    printf "%s\n" "$content" >> "$output_file"
                fi
                ;;
        esac
    done || true

    echo ""
    rm -f "$raw_file" 2>/dev/null
}

# --- Setup ---

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REQUIREMENTS_FILE="$SCRIPT_DIR/REQUIREMENTS.md"
ITERATION=0
OUTPUT_FILE="/tmp/ralph_output_$$.txt"

# Worker-specific log file
if [ -n "$WORKER_NAME" ]; then
    LOG_FILE="$SCRIPT_DIR/ralph_log_${WORKER_NAME}.md"
    STATUS_FILE="$SCRIPT_DIR/.ralph_status_${WORKER_NAME}.json"
else
    LOG_FILE="$SCRIPT_DIR/ralph_log.md"
    STATUS_FILE=""
fi

# --- Validation ---

if [ ! -f "$REQUIREMENTS_FILE" ]; then
    echo "Error: REQUIREMENTS.md not found at $REQUIREMENTS_FILE"
    exit 1
fi

if ! command -v claude &> /dev/null; then
    echo "Error: 'claude' command not found in PATH"
    exit 1
fi

if ! command -v nix &> /dev/null; then
    echo "Error: 'nix' command not found in PATH"
    exit 1
fi

# --- Build Prompt ---

if [ -n "$TASK_LIST" ]; then
    TASK_INSTRUCTION="PICK a Ralph Task from THIS SET ONLY: Tasks ${TASK_LIST}.
Do NOT work on tasks outside this set. Other workers are handling the remaining tasks.
Pick whichever task from your set is:
- Unblocked (dependencies from earlier tasks are done)
- Has failing or missing tests
- Makes progress toward the goal"
    COMPLETION_INSTRUCTION="When ALL tasks in your set (${TASK_LIST}) are complete and their tests pass: output RALPH_COMPLETE"
else
    TASK_INSTRUCTION="PICK a Ralph Task (Tasks 1-40 in REQUIREMENTS.md) that makes sense given current state:
- Unblocked (dependencies from earlier tasks are done)
- Has failing or missing tests
- Makes progress toward the goal
- Earlier task numbers generally before later ones"
    COMPLETION_INSTRUCTION="When ALL Ralph Tasks (1-40) are complete and all tests pass: output RALPH_COMPLETE"
fi

PROMPT="You are working on the Discordinator project — a Discord TUI client in Rust.

FIRST: Read REQUIREMENTS.md and CLAUDE.md to understand the project architecture and rules.
Then read ralph_log*.md files (if they exist) for previous iteration progress.

THEN: Check current state:
- What files exist in src/?
- Does \`nix develop --command cargo build\` succeed?
- Does \`nix develop --command cargo test\` pass? How many tests?
- Does \`nix develop --command cargo clippy -- -D warnings\` pass?

${TASK_INSTRUCTION}

CRITICAL RULES:
- Custom gateway (tokio-tungstenite), NOT twilight-gateway
- Custom HTTP (reqwest), NOT twilight-http
- twilight-model is OK for shared types (Id<T>, etc.)
- No Arc<Mutex<_>> — main loop owns all state, background tasks use mpsc channels
- NEVER call POST /users/@me/channels
- All commands via: nix develop --command <cmd>

WORKFLOW:
1. Write/update tests FIRST (TDD)
2. Implement minimum code to pass tests
3. Run: nix develop --command cargo test
4. Run: nix develop --command cargo clippy -- -D warnings
5. If stuck on a task, switch to a different unblocked task from your set

END your response with:
## Iteration Summary
- Task worked on: [task number and name]
- Files changed: [list]
- Tests: [X passing, Y failing]
- Clippy: [clean / N warnings]
- Next: [what to do next iteration]

${COMPLETION_INSTRUCTION}"

# --- Status Reporting (for orchestrator) ---

write_status() {
    local status="$1"
    local iteration="$2"
    local detail="${3:-}"
    if [ -n "$STATUS_FILE" ]; then
        cat > "$STATUS_FILE" <<STATUSEOF
{
  "worker": "${WORKER_NAME}",
  "tasks": "${TASK_LIST}",
  "status": "${status}",
  "iteration": ${iteration},
  "max_iterations": ${MAX_ITERATIONS},
  "timestamp": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "detail": "${detail}"
}
STATUSEOF
    fi
}

# --- Banner ---

echo "=== Ralph Wiggum Loop ==="
echo "Requirements: $REQUIREMENTS_FILE"
echo "Max iterations: $MAX_ITERATIONS"
if [ -n "$TASK_LIST" ]; then
    echo "Tasks: $TASK_LIST"
fi
if [ -n "$WORKER_NAME" ]; then
    echo "Worker: $WORKER_NAME"
fi
echo "Progress log: $LOG_FILE"
echo "========================="

# Initialize log
echo "" >> "$LOG_FILE"
echo "# Ralph Session $(date)${WORKER_NAME:+ (worker: $WORKER_NAME, tasks: $TASK_LIST)}" >> "$LOG_FILE"
echo "" >> "$LOG_FILE"

write_status "running" 0 "starting"

# --- Main Loop ---

while [ $ITERATION -lt $MAX_ITERATIONS ]; do
    ITERATION=$((ITERATION + 1))
    echo ""
    echo "=========================================="
    echo ">>> ITERATION $ITERATION of $MAX_ITERATIONS${WORKER_NAME:+ [$WORKER_NAME]}"
    echo ">>> $(date)"
    echo "=========================================="
    echo ""

    write_status "running" "$ITERATION" "iteration $ITERATION"

    run_claude_streaming "$PROMPT" "$OUTPUT_FILE"

    echo ""
    echo ">>> Finished at $(date)"

    # Check for completion signal
    if grep -q "RALPH_COMPLETE" "$OUTPUT_FILE"; then
        echo ""
        echo "=== COMPLETION SIGNAL RECEIVED ==="
        echo "Verifying build, tests, and clippy..."

        cd "$SCRIPT_DIR"
        VERIFY_PASSED=true

        echo ">>> Checking cargo build..."
        if ! nix develop --command cargo build 2>&1 | tee /tmp/ralph_build_$$.txt; then
            echo ">>> Build FAILED"
            VERIFY_PASSED=false
        fi

        echo ">>> Checking cargo test..."
        if nix develop --command cargo test 2>&1 | tee /tmp/ralph_test_$$.txt; then
            if grep -qE "FAILED|panicked" /tmp/ralph_test_$$.txt; then
                echo ">>> Tests have FAILURES"
                VERIFY_PASSED=false
            fi
        else
            echo ">>> Test run FAILED"
            VERIFY_PASSED=false
        fi

        echo ">>> Checking cargo clippy..."
        if ! nix develop --command cargo clippy -- -D warnings 2>&1 | tee /tmp/ralph_clippy_$$.txt; then
            echo ">>> Clippy has warnings/errors"
            VERIFY_PASSED=false
        fi

        rm -f /tmp/ralph_build_$$.txt /tmp/ralph_test_$$.txt /tmp/ralph_clippy_$$.txt

        if [ "$VERIFY_PASSED" = true ]; then
            echo ""
            echo "=========================================="
            echo "=== SUCCESS on iteration $ITERATION${WORKER_NAME:+ [$WORKER_NAME]} ==="
            echo "=========================================="

            # Commit worker's completed work
            if [ -n "$WORKER_NAME" ]; then
                git add -A -- ':!target/' ':!ralph_log*' ':!.ralph_status*'
                git commit -m "Worker ${WORKER_NAME}: complete tasks ${TASK_LIST}

Tasks: ${TASK_LIST}
Iterations: ${ITERATION}

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>" || true
            fi

            write_status "complete" "$ITERATION" "all tasks passed"
            exit 0
        fi

        echo ">>> Verification failed, continuing..."
    fi

    # Log iteration summary
    echo "" >> "$LOG_FILE"
    echo "### Iteration $ITERATION - $(date)" >> "$LOG_FILE"
    if grep -q "## Iteration Summary" "$OUTPUT_FILE"; then
        sed -n '/## Iteration Summary/,$p' "$OUTPUT_FILE" >> "$LOG_FILE"
    else
        echo "No summary provided" >> "$LOG_FILE"
    fi
    echo "" >> "$LOG_FILE"

    # Commit progress after each iteration (worker mode only)
    if [ -n "$WORKER_NAME" ]; then
        git add -A -- ':!target/' ':!ralph_log*' ':!.ralph_status*' 2>/dev/null || true
        git commit -m "Worker ${WORKER_NAME}: iteration ${ITERATION} progress

Co-Authored-By: Claude Opus 4.6 <noreply@anthropic.com>" 2>/dev/null || true
    fi

    echo ""
    echo ">>> Iteration $ITERATION complete, logged to $(basename "$LOG_FILE")"
    sleep 2
done

echo ""
echo "=========================================="
echo "=== MAX ITERATIONS REACHED ($MAX_ITERATIONS)${WORKER_NAME:+ [$WORKER_NAME]} ==="
echo "=========================================="
write_status "max_iterations" "$MAX_ITERATIONS" "did not complete"
exit 1
