#!/bin/sh
echo "# TODO in dq"
echo '# TODO in sq'
echo $#  # count args
x=${#y} # len
cat <<EOF
# TODO in heredoc
EOF
cat <<-'END' | sed s/a/b/ # real FIXME
# HACK in quoted heredoc
END
echo a#b
echo $((1<<n))
# BUG last
