#!/usr/bin/env bash
# Tear everything down, including any run in progress. The results bucket is kept.
#
# Usage: scripts/destroy.sh
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

select_stack
log "pulumi destroy..."
pulumi destroy --yes || true
# The bucket is kept on purpose (forceDestroy: false), so its BucketNotEmpty error is expected.
# Anything else still in the stack means the destroy really failed.
resources="$(pulumi stack export | jq -r '.deployment.resources[]?.type')" || die "could not read the stack after destroy; rerun it"
remaining="$(grep -vE '^(pulumi:pulumi:Stack|pulumi:providers:aws|aws:s3/bucket:Bucket)$' <<<"$resources" || true)"
if [[ -n "$remaining" ]]; then
    die "destroy left resources behind, rerun it: $(echo "$remaining" | sort | uniq -c | tr -s ' \n' ' ')"
fi
rm -f "$KNOWN_HOSTS" "$RESULTS_ROOT/.current-run"
log "down. only the results bucket remains"
