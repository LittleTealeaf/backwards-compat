awk '/SF:/{p=1; b=$0; next} p{b=b RS $0} /LF:/{f=$2} /LH:/{h=$2} /end_of_record/{if(f!=h) print b; p=0}' FS=: lcov_raw.info > lcov.info
sed -i "s|$GITHUB_WORKSPACE/||g" lcov.info
