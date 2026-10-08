#!/usr/bin/env bash
# Phase 0 measurement script: collect performance data for Rust vs QML backend

set -e

RESULTS_DIR="docs/performance"
TIMESTAMP=$(date +%Y%m%d-%H%M%S)
QML_LOG="$RESULTS_DIR/phase0-qml-$TIMESTAMP.log"
RUST_LOG="$RESULTS_DIR/phase0-rust-$TIMESTAMP.log"
SUMMARY="$RESULTS_DIR/phase0-$TIMESTAMP.json"

mkdir -p "$RESULTS_DIR"

echo "Phase 0: Measuring performance..."
echo "Results will be saved to $RESULTS_DIR/"

# Idle test: 60 seconds, no interaction
echo ""
echo "=== Phase 0a: Idle (60s, QML only) ==="
INIR_RUST_BACKEND=0 RUST_LOG=inir_core=debug,inir_qt=debug timeout 60 \
  ./scripts/inir run --foreground 2>&1 | tee "$QML_LOG" | grep -E 'snapshot|apply_us|pending_count|coalesced' || true

echo ""
echo "=== Phase 0a: Idle (60s, Rust backend) ==="
INIR_RUST_BACKEND=1 RUST_LOG=inir_core=debug,inir_qt=debug timeout 60 \
  ./scripts/inir run --foreground 2>&1 | tee "$RUST_LOG" | grep -E 'snapshot|apply_us|pending_count|coalesced' || true

echo ""
echo "=== Summary ==="
echo "QML-only log:   $QML_LOG"
echo "Rust-on log:    $RUST_LOG"
echo ""
echo "Quick analysis:"
echo "QML apply calls:  $(grep -c apply_us "$QML_LOG" || echo 0)"
echo "Rust apply calls: $(grep -c apply_us "$RUST_LOG" || echo 0)"
echo ""
echo "QML snapshots:    $(grep -c 'snapshot update' "$QML_LOG" || echo 0)"
echo "Rust snapshots:   $(grep -c 'snapshot update' "$RUST_LOG" || echo 0)"
echo ""
echo "To run interactive scenarios, use:"
echo "  INIR_RUST_BACKEND=0 ./scripts/inir run --foreground  # QML baseline"
echo "  # Open Wi-Fi panel, drag volume slider, switch workspaces, play media"
echo "  # Capture logs from RUST_LOG=debug"
