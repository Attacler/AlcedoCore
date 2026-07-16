#!/bin/bash
# E2E Test: ER Diagram Schema Visualization
# Tests Phase 18-21: ER diagram renders in Schema tab

set -euo pipefail

ADMIN_URL="http://localhost:8080/admin/#/plugins/hello-world"
SCREENSHOT_DIR="screenshots"
mkdir -p "$SCREENSHOT_DIR"

echo "=== E2E: ER Diagram Schema Visualization ==="

# Step 1: Open admin UI for hello-world plugin
agent-browser open "$ADMIN_URL"
echo "✓ Opened hello-world plugin page"

# Step 2: Click Schema tab
agent-browser wait --load networkidle
agent-browser snapshot -i -c
agent-browser click @e9
echo "✓ Clicked Schema tab"

# Step 3: Wait for ER diagram to render
agent-browser wait 3
SNAPSHOT=$(agent-browser snapshot 2>&1)
agent-browser screenshot "$SCREENSHOT_DIR/schema-erd.png"
echo "✓ Screenshot taken"

# Step 4: Verify ER diagram content
echo "$SNAPSHOT" | grep -q "items" && echo "✅ PASS: 'items' table visible" || echo "❌ FAIL: 'items' table not visible"
echo "$SNAPSHOT" | grep -q "PK" && echo "✅ PASS: PK marker visible" || echo "❌ FAIL: PK marker not visible"
echo "$SNAPSHOT" | grep -q "name" && echo "✅ PASS: Column 'name' visible" || echo "❌ FAIL: Column 'name' not visible"
echo "$SNAPSHOT" | grep -q "description" && echo "✅ PASS: Column 'description' visible" || echo "❌ FAIL: Column 'description' not visible"
echo "$SNAPSHOT" | grep -q "created_at" && echo "✅ PASS: Column 'created_at' visible" || echo "❌ FAIL: Column 'created_at' not visible"

echo ""
echo "=== All tests passed ==="
agent-browser close
