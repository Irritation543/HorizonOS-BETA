set -e
FONT="$1"
[ -n "$FONT" ] || { echo "usage: build_font.sh Terminus.ttf > font_data.rs" >&2; exit 1; }

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

CODES=$(seq 32 126)
CODES="$CODES 1040 1041 1042 1043 1044 1045 1025 1046 1047 1048 1049 1050 1051 1052 1053 1054 1055 1056 1057 1058 1059 1060 1061 1062 1063 1064 1065 1066 1067 1068 1069 1070 1071"
CODES="$CODES 1072 1073 1074 1075 1076 1077 1105 1078 1079 1080 1081 1082 1083 1084 1085 1086 1087 1088 1089 1090 1091 1092 1093 1094 1095 1096 1097 1098 1099 1100 1101 1102 1103"

echo "// Файл сгенерирован tools/build_font.sh из Terminus.ttf — руками не править."
echo "pub const GLYPH_W: usize = 8;"
echo "pub const GLYPH_H: usize = 16;"
n=$(echo $CODES | wc -w)
echo "pub static GLYPHS: [(u32, [u8; 16]); $n] = ["

for cp in $CODES; do
    ch=$(awk -v c="$cp" 'BEGIN{
        if (c < 128) { printf "%c", c; exit }
        if (c < 2048) {
            b1 = 192 + int(c/64); b2 = 128 + (c%64)
            printf "%c%c", b1, b2; exit
        }
        b1 = 224 + int(c/4096); b2 = 128 + int((c/64)%64); b3 = 128 + (c%64)
        printf "%c%c%c", b1, b2, b3
    }')
    convert -size 8x80 xc:black -gravity NorthWest -fill white -font "$FONT" -pointsize 16 +antialias \
        -annotate +0+32 "$ch" "$TMP/g.png"
    convert "$TMP/g.png" -crop 8x16+0+33 +repage -threshold 50% -depth 8 gray:"$TMP/g.raw"

    hex=$(od -An -v -tu1 "$TMP/g.raw" | tr -s ' \n' ' ')
    row_bytes=$(echo "$hex" | awk '
        {
            for (i = 1; i <= NF; i++) {
                bit = ($i > 127) ? 1 : 0
                byte = byte * 2 + bit
                col++
                if (col == 8) {
                    printf "0x%02X, ", byte
                    byte = 0; col = 0
                }
            }
        }')
    printf '    (0x%04X, [%s]),\n' "$cp" "$row_bytes"
done

echo "];"
