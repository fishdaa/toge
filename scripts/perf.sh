#!/usr/bin/env bash

set -euo pipefail

out_dir="perf-results"
history_root="$out_dir/history"

usage() {
    cat <<'EOF'
Usage:
  bash scripts/perf.sh run <backend> <label> <profile-scenario> [size] [iterations]
  bash scripts/perf.sh compare <backend> <label> [count]

Examples:
  bash scripts/perf.sh run perf substring-miss substring-miss
  bash scripts/perf.sh run time substring-hit substring-hit 500000 100
  bash scripts/perf.sh run heaptrack allocs substring-hit 500000 100
  bash scripts/perf.sh compare perf substring-hit 5
  bash scripts/perf.sh compare time substring-hit 5

Run writes:
  perf-results/<backend>/<label>.*
  perf-results/history/<backend>/<label>/*
EOF
}

timestamp() {
    date +"%Y%m%d-%H%M%S"
}

require_cmd() {
    local cmd="$1"
    if ! command -v "$cmd" >/dev/null 2>&1; then
        echo "required command not found: $cmd" >&2
        exit 1
    fi
}

build_profile_example() {
    cargo build --release --example profile -p needle-core >/dev/null
}

profile_binary() {
    printf '%s\n' "./target/release/examples/profile"
}

parse_profile_stdout() {
    local stdout_file="$1"
    local summary_file="$2"

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
}

parse_time_output() {
    local input_file="$1"
    local output_file="$2"

    awk '
    /^[[:space:]]*User time \(seconds\)/ {
        line = $0
        sub(/^[^:]*:[[:space:]]*/, "", line)
        user = line + 0
    }
    /^[[:space:]]*System time \(seconds\)/ {
        line = $0
        sub(/^[^:]*:[[:space:]]*/, "", line)
        sys_time = line + 0
    }
    /^[[:space:]]*Elapsed \(wall clock\) time/ {
        line = $0
        sub(/^.*\):[[:space:]]*/, "", line)
        elapsed = line
    }
    /^[[:space:]]*Maximum resident set size \(kbytes\)/ {
        line = $0
        sub(/^[^:]*:[[:space:]]*/, "", line)
        rss_kb = line + 0
    }
    /^[[:space:]]*Minor \(reclaiming a frame\) page faults/ {
        line = $0
        sub(/^[^:]*:[[:space:]]*/, "", line)
        minor_faults = line + 0
    }
    /^[[:space:]]*Major \(requiring I\/O\) page faults/ {
        line = $0
        sub(/^[^:]*:[[:space:]]*/, "", line)
        major_faults = line + 0
    }
    END {
        printf "user_seconds\t%.2f\n", user
        printf "system_seconds\t%.2f\n", sys_time
        printf "elapsed\t%s\n", elapsed
        printf "max_rss_kb\t%.0f\n", rss_kb
        printf "minor_faults\t%.0f\n", minor_faults
        printf "major_faults\t%.0f\n", major_faults
    }
    ' "$input_file" > "$output_file"
}

parse_heaptrack_report() {
    local input_file="$1"
    local output_file="$2"

    awk '
    /^total runtime:/ {
        sub(/^total runtime:[[:space:]]*/, "", $0)
        sub(/\.$/, "", $0)
        print "total_runtime\t" $0
    }
    /^calls to allocation functions:/ {
        sub(/^calls to allocation functions:[[:space:]]*/, "", $0)
        print "allocation_calls\t" $0
    }
    /^temporary memory allocations:/ {
        sub(/^temporary memory allocations:[[:space:]]*/, "", $0)
        print "temporary_memory_allocations\t" $0
    }
    /^peak heap memory consumption:/ {
        sub(/^peak heap memory consumption:[[:space:]]*/, "", $0)
        print "peak_heap_memory\t" $0
    }
    /^peak RSS \(including heaptrack overhead\):/ {
        sub(/^peak RSS \(including heaptrack overhead\):[[:space:]]*/, "", $0)
        print "peak_rss\t" $0
    }
    /^total memory leaked:/ {
        sub(/^total memory leaked:[[:space:]]*/, "", $0)
        print "total_memory_leaked\t" $0
    }
    ' "$input_file" > "$output_file"
}

compare_metric_history() {
    local suffix="$1"
    local label="$2"
    local count="${3:-5}"
    local dir="$history_root/$suffix/$label"

    if [[ ! -d "$dir" ]]; then
        echo "no history found for '$label' in $dir"
        exit 1
    fi

    mapfile -t runs < <(find "$dir" -maxdepth 1 -type f -name "*.$suffix.tsv" | sort | tail -n "$count")
    if [[ ${#runs[@]} -eq 0 ]]; then
        echo "no '$suffix' summaries found for '$label' in $dir"
        exit 1
    fi

    awk -F '\t' '
    FNR == 1 {
        file_index++
        files[file_index] = FILENAME
        sub(/^.*\//, "", files[file_index])
        sub(/\.[^.]+\.tsv$/, "", files[file_index])
    }
    {
        metric = $1
        metrics[metric] = 1
        values[metric, file_index] = $2
    }
    END {
        n = asorti(metrics, sorted)
        for (m = 1; m <= n; m++) {
            metric = sorted[m]
            printf "metric: %s\n", metric
            printf "%-24s | %14s\n", "run", "value"
            for (i = 1; i <= file_index; i++) {
                printf "%-24s | %14s\n", files[i], values[metric, i]
            }
            printf "\n"
        }
    }
    ' "${runs[@]}"
}

run_profile() {
    local backend="$1"
    local label="$2"
    shift 2

    local backend_dir="$out_dir/$backend"
    local history_dir="$history_root/$backend/$label"
    local stdout_file="$backend_dir/$label.stdout.txt"
    local summary_file="$backend_dir/$label.summary.tsv"
    local time_file="$backend_dir/$label.time.txt"
    local time_tsv_file="$backend_dir/$label.time.tsv"
    local heaptrack_tsv_file="$backend_dir/$label.heaptrack.tsv"
    local data_file="$backend_dir/$label.data"
    local report_file="$backend_dir/$label.report.txt"

    mkdir -p "$backend_dir"
    mkdir -p "$history_dir"

    build_profile_example

    local binary
    binary="$(profile_binary)"

    local ts history_prefix heaptrack_base heaptrack_output
    ts="$(timestamp)"
    history_prefix="$history_dir/$ts"

    case "$backend" in
        perf)
            require_cmd perf
            perf record -o "$data_file" --call-graph dwarf \
                "$binary" "$@" | tee "$stdout_file"
            perf report -i "$data_file" --stdio | tee "$report_file"
            cp "$data_file" "$history_prefix.data"
            cp "$report_file" "$history_prefix.report.txt"
            ;;
        time)
            require_cmd /usr/bin/time
            /usr/bin/time -v -o "$time_file" \
                "$binary" "$@" | tee "$stdout_file"
            parse_time_output "$time_file" "$time_tsv_file"
            cp "$time_file" "$history_prefix.time.txt"
            cp "$time_tsv_file" "$history_prefix.time.tsv"
            ;;
        heaptrack)
            require_cmd heaptrack
            heaptrack_base="$backend_dir/$label.heaptrack"
            heaptrack --record-only -o "$heaptrack_base" \
                "$binary" "$@" | tee "$stdout_file"

            for candidate in \
                "$heaptrack_base" \
                "$heaptrack_base.gz" \
                "$heaptrack_base.zst" \
                "$heaptrack_base.raw.gz"
            do
                if [[ -f "$candidate" ]]; then
                    heaptrack_output="$candidate"
                    break
                fi
            done

            if [[ -z "${heaptrack_output:-}" ]]; then
                echo "heaptrack completed but no output file was found near $heaptrack_base" >&2
                exit 1
            fi

            local heaptrack_suffix
            heaptrack_suffix="${heaptrack_output#"$heaptrack_base"}"
            cp "$heaptrack_output" "$history_prefix.heaptrack$heaptrack_suffix"
            if command -v heaptrack_print >/dev/null 2>&1; then
                heaptrack_print -f "$heaptrack_output" > "$report_file"
                awk '
                /^calls to allocation functions:/ ||
                /^temporary memory allocations:/ ||
                /^peak heap memory consumption:/ ||
                /^peak RSS \(including heaptrack overhead\):/ ||
                /^total memory leaked:/ ||
                /^total runtime:/ { print }
                ' "$report_file" > "$heaptrack_tsv_file.raw"
                parse_heaptrack_report "$heaptrack_tsv_file.raw" "$heaptrack_tsv_file"
                rm -f "$heaptrack_tsv_file.raw"
                cp "$report_file" "$history_prefix.report.txt"
                cp "$heaptrack_tsv_file" "$history_prefix.heaptrack.tsv"
            fi
            ;;
        *)
            echo "unsupported backend: $backend" >&2
            usage
            exit 1
            ;;
    esac

    parse_profile_stdout "$stdout_file" "$summary_file"

    cp "$stdout_file" "$history_prefix.stdout.txt"
    cp "$summary_file" "$history_prefix.summary.tsv"

    printf 'saved %s run to %s.*\n' "$backend" "$history_prefix"
}

compare_history() {
    local backend="${1:-}"
    local label="${2:-}"
    local count="${3:-5}"

    if [[ -z "$backend" || -z "$label" ]]; then
        usage
        exit 1
    fi

    compare_metric_history "summary" "$label" "$count"
    if [[ "$backend" == "time" ]]; then
        compare_metric_history "time" "$label" "$count"
    elif [[ "$backend" == "heaptrack" ]]; then
        compare_metric_history "heaptrack" "$label" "$count"
    fi
}

main() {
    if [[ $# -lt 1 ]]; then
        usage
        exit 1
    fi

    case "$1" in
        run)
            if [[ $# -lt 4 ]]; then
                usage
                exit 1
            fi
            shift
            run_profile "$@"
            ;;
        compare)
            if [[ $# -lt 3 ]]; then
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
