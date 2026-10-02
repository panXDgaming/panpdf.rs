#!/bin/sh
set -eu
for arg in "$@"; do
    case "$arg" in
        --list-langs)
            printf 'List of available languages in "/fake/" (3):\neng\nlao\ntha\n'
            exit 0
            ;;
        --version)
            printf 'tesseract 5.3.0\n'
            exit 0
            ;;
    esac
done
base=
previous=
for arg in "$@"; do
    if [ "$previous" = stdin ]; then
        base=$arg
    fi
    previous=$arg
done
if [ -z "$base" ]; then
    printf 'fake-tesseract: no output name was given\n' >&2
    exit 1
fi
cat > /dev/null
{
    printf 'level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext\n'
    printf '1\t1\t0\t0\t0\t0\t0\t0\t2479\t3508\t-1\t\n'
    printf '4\t1\t1\t1\t1\t0\t300\t300\t900\t60\t-1\t\n'
    printf '5\t1\t1\t1\t1\t1\t300\t300\t250\t60\t90\tຍິນດີ\n'
    printf '5\t1\t1\t1\t1\t2\t580\t300\t300\t60\t90\tຕ້ອນຮັບ\n'
    printf '5\t1\t1\t1\t1\t3\t910\t300\t290\t60\t90\tWelcome\n'
} > "$base.tsv"
printf 'ຍິນດີ ຕ້ອນຮັບ Welcome\n' > "$base.txt"
