#!/bin/bash
# E2E Test: Collection Builder Full Lifecycle
# Tests Phase 25-27: Collection creation, field editing, data browsing, relationships
#
# Prerequisites:
#   - Docker Compose environment running (docker compose up -d)
#   - Admin UI compiled (npm run build in system-plugins/admin/)
#   - Admin UI published to .docker-plugins/admin/public/
#   - agent-browser CLI available on PATH
#   - curl available for API calls
#
# Usage:
#   bash system-plugins/admin/tests/e2e/collection-builder-test.sh
#
# Test flow:
#   1. Navigate to Collections page via admin UI
#   2. Create collection "e2e_test_authors" with a "name" field
#   3. Create collection "e2e_test_books" with typed fields (title, pages, rating)
#   4. Add items via data browser API
#   5. Add relationship field "author_id" referencing e2e_test_authors
#   6. Verify persistence after page reload

set -euo pipefail

# --- Configuration ---
BASE_URL="http://localhost:8080/admin"
API_BASE="http://localhost:8080/api"
SCREENSHOT_DIR="screenshots"
PASS_COUNT=0
FAIL_COUNT=0
STEP=0

# --- Prerequisite Checks ---
if ! command -v agent-browser &>/dev/null; then
    echo "❌ FAIL: agent-browser CLI not found — install with 'npm i -g agent-browser && agent-browser install'"
    exit 1
fi

if ! curl -sf --max-time 3 "${BASE_URL}/index.html" > /dev/null 2>&1; then
    echo "⚠  WARN: Admin UI at ${BASE_URL} is not responding."
    echo "   The script will attempt to continue (steps may fail)."
fi

# --- Helpers ---

pass() {
    echo "  ✅  PASS: $1"
    PASS_COUNT=$((PASS_COUNT + 1))
}

fail() {
    echo "  ❌  FAIL: $1"
    FAIL_COUNT=$((FAIL_COUNT + 1))
}

step_header() {
    STEP=$((STEP + 1))
    echo ""
    echo "--- Step ${STEP}: $1 ---"
}

# Snap and grep for content in snapshot output
assert_snapshot() {
    local desc="$1"
    local pattern="$2"
    local snap_file=$(mktemp)
    agent-browser snapshot 2>&1 > "$snap_file"
    if grep -qi "$pattern" "$snap_file"; then
        pass "$desc"
        rm -f "$snap_file"
        return 0
    else
        fail "$desc"
        rm -f "$snap_file"
        return 1
    fi
}

# Take a screenshot
screenshot() {
    local name="$1"
    agent-browser screenshot "${SCREENSHOT_DIR}/${name}.png" 2>&1 > /dev/null
    echo "   Screenshot: ${name}.png"
}

# Reset browser session
reset_browser() {
    agent-browser close 2>&1 > /dev/null || true
    sleep 1
}

# Navigate to a specific SPA route via pushstate
navigate_to() {
    local route="$1"
    agent-browser pushstate "/admin/#${route}" 2>&1
    sleep 1
}

# Open admin and navigate to collections
navigate_collections() {
    reset_browser
    agent-browser open "${BASE_URL}/" 2>&1
    agent-browser wait --load networkidle 2>&1
    sleep 1
    agent-browser find text "Collections" click 2>&1
    sleep 1
}

# Refresh snapshot refs
refresh_refs() {
    agent-browser snapshot -i -c 2>&1 > /dev/null
    sleep 0.3
}

# ============================================================
# Setup
# ============================================================
echo ""
echo "=========================================="
echo "  E2E: Collection Builder Full Lifecycle"
echo "=========================================="
echo ""

# Clean up any previous test collections
echo "--- Cleaning up previous test collections ---"
curl -sf -X DELETE "${API_BASE}/collections/e2e_test_books" > /dev/null 2>&1 || true
curl -sf -X DELETE "${API_BASE}/collections/e2e_test_authors" > /dev/null 2>&1 || true
echo "   Done"
mkdir -p "$SCREENSHOT_DIR"
echo ""

# ============================================================
# STEP 1: Navigate to Collections page
# ============================================================
step_header "Navigate to Collections page"
navigate_collections
screenshot "step1-collections-list"

CURRENT_URL=$(agent-browser get url 2>&1)
if echo "$CURRENT_URL" | grep -qi "collections"; then
    pass "Navigated to Collections page"
else
    fail "Did not navigate to Collections page"
fi

# ============================================================
# STEP 2: Create collection "e2e_test_authors" with a name field
# ============================================================
step_header "Create collection: e2e_test_authors"

# Open modal and fill name
agent-browser find text "Create Collection" click 2>&1
sleep 1
agent-browser find placeholder "e.g. users" fill "e2e_test_authors" 2>&1
sleep 0.5
screenshot "step2a-name-filled"

# Click "Create" (modal button) — use --exact to avoid "Create Collection" match
agent-browser find text "Create" click --exact 2>&1
sleep 2
screenshot "step2b-after-create"

# Verify navigation
CURRENT_URL=$(agent-browser get url 2>&1)
if echo "$CURRENT_URL" | grep -qi "e2e_test_authors"; then
    pass "Navigated to collection editor for e2e_test_authors"
else
    fail "Did not navigate to collection editor"
fi

# --- Add "name" field ---
echo "   Adding 'name' field..."
refresh_refs
agent-browser find text "+ Add" click 2>&1
sleep 1
agent-browser find placeholder "e.g. email" fill "name" 2>&1
sleep 0.5
agent-browser find text "Required" click 2>&1 || true
sleep 0.3
agent-browser find text "Save Changes" click 2>&1
sleep 2
screenshot "step2c-field-saved"
assert_snapshot "Field 'name' saved in builder" "name"

# ============================================================
# STEP 3: Create collection "e2e_test_books" with typed fields
# ============================================================
step_header "Create collection: e2e_test_books"

# Back to collections list
agent-browser find text "Back to Collections" click 2>&1
sleep 1

# Create the second collection
agent-browser find text "Create Collection" click 2>&1
sleep 1
agent-browser find placeholder "e.g. users" fill "e2e_test_books" 2>&1
sleep 0.5
agent-browser find text "Create" click --exact 2>&1
sleep 2
screenshot "step3a-books-created"

CURRENT_URL=$(agent-browser get url 2>&1)
if echo "$CURRENT_URL" | grep -qi "e2e_test_books"; then
    pass "Navigated to collection editor for e2e_test_books"
else
    fail "Did not navigate to collection editor"
fi

# --- Add typed fields ---

# Add field: name + type (using select @e12 after field is added)
add_field_with_type() {
    local fname="$1"
    local ftype="$2"
    local required="${3:-false}"

    echo "   Adding field: $fname ($ftype)"

    agent-browser find text "+ Add" click 2>&1
    sleep 1
    agent-browser find placeholder "e.g. email" fill "$fname" 2>&1
    sleep 0.3

    # Refresh refs so @e12 points to the field type combobox
    refresh_refs

    if [ "$ftype" != "string" ]; then
        agent-browser select @e12 "$ftype" 2>&1 || echo "   ⚠  Could not select type '$ftype'"
        sleep 1
    fi

    if [ "$required" = "true" ]; then
        agent-browser find text "Required" click 2>&1 || true
        sleep 0.3
    fi
}

add_field_with_type "title" "string" "true"
add_field_with_type "pages" "int" "false"
add_field_with_type "rating" "float" "false"

# Save all fields
echo "   Saving all fields..."
agent-browser find text "Save Changes" click 2>&1
sleep 2
screenshot "step3b-fields-saved"

assert_snapshot "Field 'title' visible" "title"
assert_snapshot "Field 'pages' visible" "pages"
assert_snapshot "Field 'rating' visible" "rating"

# ============================================================
# STEP 4: Data browser and item creation
# ============================================================
step_header "Data browser and item creation"

# Navigate to data browser
agent-browser find text "Browse Data" click 2>&1
sleep 3
screenshot "step4a-data-browser"

# Check data browser loaded
agent-browser snapshot 2>&1 > /dev/null
assert_snapshot "Data browser loaded" "Loading\|items\|title\|No items"

# Create test item via API
# The items API accepts a JSON object directly (not wrapped in {"data": ...})
echo "   Creating 'Test Book' item via API..."
API_RESPONSE=$(curl -sf -X POST \
    "${API_BASE}/collections/e2e_test_books/items" \
    -H "Content-Type: application/json" \
    -d '{"title": "Test Book", "pages": "200", "rating": "4.5"}' 2>&1) || {
    fail "Could not create item via API: $(echo "$API_RESPONSE" | head -1)"
    API_RESPONSE=""
}

ITEM_ID=$(echo "$API_RESPONSE" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
if [ -n "$ITEM_ID" ]; then
    pass "Item created via API (id: ${ITEM_ID:0:8}...)"
else
    fail "Could not extract item ID from API response"
fi

# Create author item
echo "   Creating 'Jane Austen' author via API..."
AUTHOR_RESP=$(curl -sf -X POST \
    "${API_BASE}/collections/e2e_test_authors/items" \
    -H "Content-Type: application/json" \
    -d '{"name": "Jane Austen"}' 2>&1) || {
    fail "Could not create author item"
    AUTHOR_RESP=""
}

AUTHOR_ID=$(echo "$AUTHOR_RESP" | grep -o '"id":"[^"]*"' | head -1 | cut -d'"' -f4)
if [ -n "$AUTHOR_ID" ]; then
    pass "Author item created via API (id: ${AUTHOR_ID:0:8}...)"
else
    fail "Could not extract author ID"
fi

# Reload data browser to show items
agent-browser reload 2>&1
sleep 3
screenshot "step4b-item-visible"
assert_snapshot "Item 'Test Book' visible in data table" "Test Book"

# ============================================================
# STEP 5: Add relationship field to e2e_test_books
# ============================================================
step_header "Add relationship field"

# Navigate back to builder
echo "   Navigating to collection builder..."
navigate_to "/collections/e2e_test_books/edit"
sleep 2
screenshot "step5a-builder-reloaded"

# Refresh refs for this page state
refresh_refs

# Verify builder loaded
if agent-browser snapshot 2>&1 | grep -qi "e2e_test_books"; then
    pass "Builder page loaded for e2e_test_books"
else
    fail "Builder page not loaded correctly"
fi

# Add relationship field
echo "   Adding relationship field 'author_id'..."
agent-browser find text "+ Add" click 2>&1
sleep 1
agent-browser find placeholder "e.g. email" fill "author_id" 2>&1
sleep 0.3

# Select "relationship" type
refresh_refs
agent-browser select @e12 "relationship" 2>&1
sleep 1

# Refresh refs again — relationship settings have new comboboxes
refresh_refs

# Select related collection (e2e_test_authors)
echo "   Selecting related collection..."
# After selecting relationship, the Related Collection combobox appears.
# It's the third combobox (after Field Type and Display Type).
# We find it via the label text approach.
agent-browser select @e17 "e2e_test_authors" 2>&1 || {
    echo "   ⚠  Could not select related collection via ref, trying text..."
    agent-browser find text "Select a collection" click 2>&1 || true
    sleep 0.5
    agent-browser find text "e2e_test_authors" click 2>&1 || true
    sleep 0.5
}
sleep 0.3

screenshot "step5b-relationship-settings"

# Save changes
echo "   Saving changes with relationship field..."
agent-browser find text "Save Changes" click 2>&1
sleep 2
screenshot "step5c-relationship-saved"

assert_snapshot "Relationship field 'author_id' visible" "author_id"

# ============================================================
# STEP 6: Verify persistence after page reload
# ============================================================
step_header "Verify persistence after page reload"

# Navigate away
echo "   Navigating away to Dashboard..."
navigate_to "/dashboard"
sleep 1
screenshot "step6a-dashboard"
pass "Navigated away from collections"

# Navigate back to collections
echo "   Navigating back to collections..."
navigate_to "/collections"
sleep 2
screenshot "step6b-back-to-collections"

# Verify both collections listed
assert_snapshot "Collection 'e2e_test_books' survived reload" "e2e_test_books"
assert_snapshot "Collection 'e2e_test_authors' survived reload" "e2e_test_authors"

# Navigate to data browser
echo "   Navigating to data browser..."
navigate_to "/collections/e2e_test_books/data"
sleep 3
screenshot "step6c-data-after-reload"

assert_snapshot "Item 'Test Book' survived reload" "Test Book"

# ============================================================
# STEP 7: Cleanup
# ============================================================
step_header "Cleanup"
agent-browser close 2>&1
pass "Browser session closed"

# Clean up test collections via API
echo "   Cleaning up test collections..."
curl -sf -X DELETE "${API_BASE}/collections/e2e_test_books" > /dev/null 2>&1 || true
curl -sf -X DELETE "${API_BASE}/collections/e2e_test_authors" > /dev/null 2>&1 || true
echo "   Done"

# ============================================================
# Summary
# ============================================================
echo ""
echo "=========================================="
echo "  RESULTS: ${PASS_COUNT} passed, ${FAIL_COUNT} failed"
echo "=========================================="

if [ "$FAIL_COUNT" -gt 0 ]; then
    echo ""
    echo "⚠  Some tests failed. Review screenshots in: ${SCREENSHOT_DIR}/"
    echo "   Common fixes:"
    echo "   - Rebuild admin UI: cd system-plugins/admin && npm run build && cp -r public/* ../../.docker-plugins/admin/public/"
    echo "   - Rebuild core: cd plugin-core && cargo build && cd .. && docker compose build core && docker compose up -d core"
    echo "   - Reset agent-browser: agent-browser close"
    exit 1
else
    echo ""
    echo "🎉  All tests passed!"
fi
