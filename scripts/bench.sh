#!/usr/bin/env bash

set -euo pipefail

out_dir="bench-results"
history_dir="$out_dir/history"

usage() {
    cat <<'EOF'
Usage:
  bash scripts/bench.sh run [label]
  bash scripts/bench.sh compare [count]

Examples:
  bash scripts/bench.sh run baseline
  bash scripts/bench.sh compare 5
EOF
}

timestamp() {
    date +"%Y%m%d-%H%M%S"
}

sanitize_label() {
    printf '%s' "$1" | tr ' /' '__'
}

parse_bench_output() {
    local input_file="$1"
    local output_file="$2"

    awk '
    function to_us(val, unit) {
        gsub(/[^0-9.]/, "", val)
        if (unit ~ /µs|us/) return val + 0
        if (unit ~ /ms/) return (val + 0) * 1000
        if (unit ~ /s/)  return (val + 0) * 1000000
        return val + 0
    }
    function print_row(metric, us, aux) {
        printf "%s\t%.1f\t%s\n", metric, us, aux
    }
    /^insert[[:space:]]+[0-9]+ entries:/ {
        size=$2; val=$4; unit=$5
        print_row("insert-" size, to_us(val, unit), $6)
    }
    /^substr miss[[:space:]]+[0-9]+ entries:/ {
        size=$3; val=$5; unit=$6
        print_row("substr-miss-" size, to_us(val, unit), $7)
    }
    /^substr hit[[:space:]]+[0-9]+ entries:/ {
        size=$3; val=$5; unit=$6
        print_row("substr-hit-" size, to_us(val, unit), $7)
    }
    /^prefix[[:space:]]+[0-9]+ entries:/ {
        size=$2; val=$4; unit=$5
        print_row("prefix-" size, to_us(val, unit), $6)
    }
    /^  save:/ {
        val=$2; unit=$3
        print_row("persistence-save", to_us(val, unit), $4)
    }
    /^  load:/ {
        val=$2; unit=$3
        print_row("persistence-load", to_us(val, unit), "")
    }
    /^walk[[:space:]]+/ {
        val=$4; unit=$5
        print_row("walk-synthetic", to_us(val, unit), $6)
    }
    ' "$input_file" > "$output_file"
}

run_bench() {
    local label="${1:-}"
    local ts base_name text_file tsv_file

    mkdir -p "$history_dir"
    ts="$(timestamp)"
    base_name="$ts"
    if [[ -n "$label" ]]; then
        base_name="${base_name}-$(sanitize_label "$label")"
    fi

    text_file="$history_dir/$base_name.txt"
    tsv_file="$history_dir/$base_name.tsv"

    cargo run --release --example bench -p needle-core | tee "$text_file"
    parse_bench_output "$text_file" "$tsv_file"

    cp "$text_file" "$out_dir/latest.txt"
    cp "$tsv_file" "$out_dir/latest.tsv"

    printf 'saved bench run to %s and %s\n' "$text_file" "$tsv_file"
}

compare_bench() {
    local count="${1:-5}"
    mapfile -t runs < <(find "$history_dir" -maxdepth 1 -type f -name '*.tsv' | sort | tail -n "$count")

    if [[ ${#runs[@]} -eq 0 ]]; then
        echo "no bench history found in $history_dir"
        exit 1
    fi

    awk -F '\t' '
    function fmt_us(us) {
        if (us < 1000) return sprintf("%4.0f µs", us)
        if (us < 1000000) return sprintf("%7.2f ms", us / 1000)
        return sprintf("%7.2f  s", us / 1000000)
    }
    FNR == 1 {
        file_index++
        files[file_index] = FILENAME
        sub(/^.*\//, "", files[file_index])
        sub(/\.tsv$/, "", files[file_index])
    }
    {
        key = $1
        metrics[key] = 1
        values[key, file_index] = $2 + 0
    }
    END {
        printf "%-24s", "metric"
        for (i = 1; i <= file_index; i++) {
            printf " | %14s", files[i]
        }
        printf "\n"

        n = asorti(metrics, sorted)
        for (m = 1; m <= n; m++) {
            key = sorted[m]
            printf "%-24s", key
            for (i = 1; i <= file_index; i++) {
                if ((key, i) in values) {
                    if (i == 1 || !((key, i - 1) in values) || values[key, i - 1] == 0) {
                        printf " | %7s %4s", fmt_us(values[key, i]), "-"
                    } else {
                        delta = ((values[key, i] - values[key, i - 1]) / values[key, i - 1]) * 100
                        printf " | %7s %+.1f%%", fmt_us(values[key, i]), delta
                    }
                } else {
                    printf " | %12s", "-"
                }
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
            shift
            run_bench "${1:-}"
            ;;
        compare)
            shift
            compare_bench "${1:-5}"
            ;;
        *)
            usage
            exit 1
            ;;
    esac
}

main "$@"
