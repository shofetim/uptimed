#!/bin/sh

# Run as root!
if [ "$(id -u)" -ne 0 ]
then
    echo "Please run as root"
    exit
fi

if [ $# -ne 4 ]
then
   echo "You must supply four arguments hostname-to-report-to namespace filesytem network-interface"
   exit
fi

killall uptimed
curl -O https://jordanschatz.com/uptimed/uptimed
chmod +x uptimed
mv uptimed /usr/local/bin/

# $1 Hostname to report metrics to
# $2 Namespace
# $3 Filesystem
# $4 Network interface name
uptimed "$1" "$2" "$3" "$4"

cat <<EOF > /etc/rc.local
#!/bin/sh

uptimed $1 $2 $3 $4

EOF

chmod +x /etc/rc.local
