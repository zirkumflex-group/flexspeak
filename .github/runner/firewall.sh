#!/bin/sh
# The CI VM may reach the internet but nothing on the LAN or on this host.
set -e
NET=10.88.0.0/24
for c in DOCKER-USER INPUT; do
  while iptables -D "$c" -s "$NET" -j FLEXSPEAK-CI 2>/dev/null; do :; done
done
iptables -N FLEXSPEAK-CI 2>/dev/null || iptables -F FLEXSPEAK-CI
iptables -A FLEXSPEAK-CI -m conntrack --ctstate ESTABLISHED,RELATED -j RETURN
for dst in 10.0.0.0/8 172.16.0.0/12 192.168.0.0/16 169.254.0.0/16 100.64.0.0/10 127.0.0.0/8; do
  iptables -A FLEXSPEAK-CI -d "$dst" -j DROP
done
iptables -A FLEXSPEAK-CI -m addrtype --dst-type LOCAL -j DROP
iptables -A FLEXSPEAK-CI -j RETURN
iptables -I DOCKER-USER 1 -s "$NET" -j FLEXSPEAK-CI
iptables -I INPUT 1 -s "$NET" -j FLEXSPEAK-CI
