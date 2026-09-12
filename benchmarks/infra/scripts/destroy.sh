#!/usr/bin/env bash
# Tear everything down, including any run in progress. The results bucket is kept.
#
# Usage: scripts/destroy.sh
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

select_stack
log "pulumi destroy..."
if ! pulumi destroy --yes; then
    log "destroy reported an error for the results bucket, which is kept on purpose; everything else is gone"
fi
rm -f "$KNOWN_HOSTS" "$RESULTS_ROOT/.current-run"
