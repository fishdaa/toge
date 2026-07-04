#!/usr/bin/env bash

set -euo pipefail

out_dir="perf-results"
history_root="$out_dir/history"

usage() {
    cat <<'EOF'
Usage:
  bash scripts/perf.sh run <label> <profile-scenario> [size] [iterations]
  bash scripts/perf.sh compare <label> [count]

Examples:
  bash scripts/perf.sh run substring-miss substring-miss
  bash scripts/perf.sh run substring-hit substring-hit 500000 100
  bash scripts/perf.sh compare substring-hit 5

Run writes:
  perf-results/<label>.data
  perf-results/<label>.report.txt
  perf-results/history/<label>/*.summary.tsv
EOF
}

run_profile() {
    local label="$1"
    shift

    local data_file="$out_dir/$label.data"
    local report_file="$out_dir/$label.report.txt"
    local stdout_file="$out_dir/$label.stdout.txt"
    local summary_file="$out_dir/$label.summary.tsv"
    local history_dir="$history_root/$label"

    mkdir -p "$out_dir"
    mkdir -p "$history_dir"

    local timestamp history_prefix
    timestamp="$(date +"%Y%m%d-%H%M%S")"
    history_prefix="$history_dir/$timestamp"

    perf record -o "$data_file" --call-graph dwarf \
        cargo run --release --example profile -p needle-core -- "$@" | tee "$stdout_file"

    perf report -i "$data_file" --stdio | tee "$report_file"

    awk '
    /^[[:space:]]*[[:alnum:]-]+:[[:space:]]+[0-9.]+ ms total/ {
        metric=$1
        sub(/:$/, "", metric)
        ms=$2 + 0
        units=$6 + 0
        rate=$9 + 0
        printf "%s\t%.1f\t%.0f\t%.0f\n", metric, ms, units, rate
    }
    ' "$stdout_file" > "$summary_file"

    cp "$data_file" "$history_prefix.data"
    cp "$report_file" "$history_prefix.report.txt"
    cp "$stdout_file" "$history_prefix.stdout.txt"
    cp "$summary_file" "$history_prefix.summary.tsv"

    printf 'saved perf run to %s.*\n' "$history_prefix"
}

compare_history() {
    local label="${1:-}"
    local count="${2:-5}"
    local dir="$history_root/$label"

    if [[ -z "$label" ]]; then
        usage
        exit 1
    fi

    if [[ ! -d "$dir" ]]; then
        echo "no perf history found for label '$label' in $dir"
        exit 1
    fi

    mapfile -t runs < <(find "$dir" -maxdepth 1 -type f -name '*.summary.tsv' | sort | tail -n "$count")
    if [[ ${#runs[@]} -eq 0 ]]; then
        echo "no perf summaries found for label '$label' in $dir"
        exit 1
    fi

    awk -F '\t' '
    FNR == 1 {
        file_index++
        files[file_index] = FILENAME
        sub(/^.*\//, "", files[file_index])
        sub(/\.summary\.tsv$/, "", files[file_index])
    }
    {
        metric = $1
        metrics[metric] = 1
        ms[metric, file_index] = $2 + 0
        units[metric, file_index] = $3 + 0
        rate[metric, file_index] = $4 + 0
    }
    END {
        n = asorti(metrics, sorted)
        for (m = 1; m <= n; m++) {
            metric = sorted[m]
            printf "metric: %s\n", metric
            printf "%-24s | %12s | %12s | %12s\n", "run", "ms_total", "units", "units_per_s"
            for (i = 1; i <= file_index; i++) {
                printf "%-24s | %7.1f ", files[i], ms[metric, i]
                if (i == 1 || ms[metric, i - 1] == 0) {
                    printf "%4s", "-"
                } else {
                    delta = ((ms[metric, i] - ms[metric, i - 1]) / ms[metric, i - 1]) * 100
                    printf "%+.1f%%", delta
                }
                printf " | %12.0f | %12.0f\n", units[metric, i], rate[metric, i]
            }
            printf "\n"
        }
    }
    ' "${runs[@]}"
}

main() {
    if [[ $# -lt 1 ]]; then
        usage
        exit 1
    fi

    case "$1" in
        run)
            if [[ $# -lt 3 ]]; then
                usage
                exit 1
            fi
            shift
            run_profile "$@"
            ;;
        compare)
            if [[ $# -lt 2 ]]; then
                usage
                exit 1
            fi
            shift
            compare_history "$@"
            ;;
        *)
            usage
            exit 1
            ;;
    esac
}

main "$@"
