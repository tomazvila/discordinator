#!/bin/bash

# Ralph Wiggum Loop for Discordinator Development
# Reads REQUIREMENTS.md and works through tasks until all pass
# Usage: ./ralph.sh [max_iterations]

# Ensure claude and nix are in PATH
export PATH="$HOME/.local/bin:$HOME/.nix-profile/bin:/nix/var/nix/profiles/default/bin:/opt/homebrew/bin:$PATH"

# --- Helper Functions ---

# Run claude with real-time streaming output using --output-format stream-json
run_claude_streaming() {
    local prompt="$1"
    local output_file="$2"
    local raw_file="/tmp/ralph_raw_$$.json"

    echo ">>> Streaming Claude output..."
    echo ""

    # Clear output file
    > "$output_file"

    if command -v jq &> /dev/null; then
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
    else
        echo "ERROR: jq is required. It should be available via nix develop."
        exit 1
    fi

    echo ""
    rm -f "$raw_file" 2>/dev/null
}

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REQUIREMENTS_FILE="$SCRIPT_DIR/REQUIREMENTS.md"
MAX_ITERATIONS="${1:-50}"
ITERATION=0
OUTPUT_FILE="/tmp/ralph_output_$$.txt"
LOG_FILE="$SCRIPT_DIR/ralph_log.md"

if [ ! -f "$REQUIREMENTS_FILE" ]; then
    echo "Error: REQUIREMENTS.md not found at $REQUIREMENTS_FILE"
    exit 1
fi

# Check claude is available
if ! command -v claude &> /dev/null; then
    echo "Error: 'claude' command not found in PATH"
    echo "PATH: $PATH"
    exit 1
fi

# Check nix is available
if ! command -v nix &> /dev/null; then
    echo "Error: 'nix' command not found in PATH"
    exit 1
fi

echo "=== Ralph Wiggum Loop ==="
echo "Requirements: $REQUIREMENTS_FILE"
echo "Max iterations: $MAX_ITERATIONS"
echo "Progress log: $LOG_FILE"
echo "========================="

# Initialize or append to log
echo "" >> "$LOG_FILE"
echo "# Ralph Session $(date)" >> "$LOG_FILE"
echo "" >> "$LOG_FILE"

PROMPT="You are working on the Discordinator project — a Discord TUI client in Rust.

FIRST: Read REQUIREMENTS.md and CLAUDE.md to understand the project architecture and rules.
Then read ralph_log.md (if it exists) for previous iteration progress.

THEN: Check current state:
- What files exist in src/?
- Does \`nix develop --command cargo build\` succeed?
- Does \`nix develop --command cargo test\` pass? How many tests?
- Does \`nix develop --command cargo clippy -- -D warnings\` pass?

PICK a Ralph Task (Tasks 1-40 in REQUIREMENTS.md) that makes sense given current state:
- Unblocked (dependencies from earlier tasks are done)
- Has failing or missing tests
- Makes progress toward the goal
- Earlier task numbers generally before later ones

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
5. If stuck on a task, switch to a different unblocked task

END your response with:
## Iteration Summary
- Task worked on: [task number and name]
- Files changed: [list]
- Tests: [X passing, Y failing]
- Clippy: [clean / N warnings]
- Next: [what to do next iteration]

When ALL Ralph Tasks (1-40) are complete and all tests pass: output RALPH_COMPLETE"

while [ $ITERATION -lt $MAX_ITERATIONS ]; do
    ITERATION=$((ITERATION + 1))
    echo ""
    echo "=========================================="
    echo ">>> ITERATION $ITERATION of $MAX_ITERATIONS"
    echo ">>> $(date)"
    echo "=========================================="
    echo ""

    echo ">>> Started at $(date)"
    echo ""

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

        # Verify cargo build
        echo ">>> Checking cargo build..."
        if ! nix develop --command cargo build 2>&1 | tee /tmp/ralph_build.txt; then
            echo ">>> Build FAILED"
            VERIFY_PASSED=false
        fi

        # Verify cargo test
        echo ">>> Checking cargo test..."
        if nix develop --command cargo test 2>&1 | tee /tmp/ralph_test.txt; then
            if grep -qE "FAILED|panicked" /tmp/ralph_test.txt; then
                echo ">>> Tests have FAILURES"
                VERIFY_PASSED=false
            fi
        else
            echo ">>> Test run FAILED"
            VERIFY_PASSED=false
        fi

        # Verify clippy
        echo ">>> Checking cargo clippy..."
        if ! nix develop --command cargo clippy -- -D warnings 2>&1 | tee /tmp/ralph_clippy.txt; then
            echo ">>> Clippy has warnings/errors"
            VERIFY_PASSED=false
        fi

        if [ "$VERIFY_PASSED" = true ]; then
            echo ""
            echo "=========================================="
            echo "=== SUCCESS on iteration $ITERATION ==="
            echo "=========================================="
            exit 0
        fi

        echo ">>> Verification failed, continuing..."
    fi

    # Extract and log the iteration summary
    echo "" >> "$LOG_FILE"
    echo "### Iteration $ITERATION - $(date)" >> "$LOG_FILE"
    if grep -q "## Iteration Summary" "$OUTPUT_FILE"; then
        sed -n '/## Iteration Summary/,$p' "$OUTPUT_FILE" >> "$LOG_FILE"
    else
        echo "No summary provided" >> "$LOG_FILE"
    fi
    echo "" >> "$LOG_FILE"

    echo ""
    echo ">>> Iteration $ITERATION complete, logged to ralph_log.md"
    sleep 2
done

echo ""
echo "=========================================="
echo "=== MAX ITERATIONS REACHED ($MAX_ITERATIONS) ==="
echo "=========================================="
exit 1
