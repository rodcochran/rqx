#!/usr/bin/env bash
# Copy a run down from the client and upload it to the results bucket. With --archive, also
# copy it to benchmarks/results/aws-<date>-v<version>/ and render the charts into benchmarks/<version>/.
#
# Usage: scripts/collect.sh [RUN_ID] [--archive VERSION]
. "$(dirname "${BASH_SOURCE[0]}")/common.sh"

run_arg=""
archive=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --archive) archive="$2"; shift 2 ;;
        *) run_arg="$1"; shift ;;
    esac
done

resolve_run "$run_arg"
select_stack
read_outputs
sync_down
run_finished || log "note: the run has not finished; this is a partial copy"
log "results at $LOCAL_RESULTS"

if aws s3 sync --quiet "$LOCAL_RESULTS/" "s3://$BUCKET/$RUN_ID/"; then
    log "uploaded to s3://$BUCKET/$RUN_ID/"
else
    log "WARNING: S3 upload failed; the local copy is complete"
fi

if [[ -n "$archive" ]]; then
    dest="$REPO_DIR/benchmarks/results/aws-$(date -u +%Y%m%d)-v${archive//./}"
    rm -rf "$dest"
    cp -R "$LOCAL_RESULTS" "$dest"
    log "archived to $dest"
    (cd "$REPO_DIR" && "$(repo_python)" benchmarks/plot_bench.py "$dest" --out-dir "benchmarks/$archive")
    log "charts in benchmarks/$archive/"
fi
