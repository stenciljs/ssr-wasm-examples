#!/bin/sh
# Runs a build command (Maven, Fetch.java) trusting the root CA passed as the optional `ca`
# BuildKit secret, for builds behind a TLS-inspecting proxy. Without the secret it just runs
# the command. The CA goes into a throwaway copy of the JDK truststore that's deleted
# afterwards, so it never ends up in an image layer.
set -eu
if [ -s /run/secrets/ca ]; then
  store=$(mktemp -d)
  trap 'rm -rf "$store"' EXIT
  cp "$JAVA_HOME/lib/security/cacerts" "$store/cacerts"
  # keytool imports one certificate per call; the PEM may hold a chain.
  awk -v dir="$store" '/BEGIN CERTIFICATE/ { n++ } n { print > (dir "/ca-" n ".pem") }' /run/secrets/ca
  for pem in "$store"/ca-*.pem; do
    keytool -importcert -noprompt -alias "proxy-$(basename "$pem" .pem)" -file "$pem" \
      -keystore "$store/cacerts" -storepass changeit >/dev/null
  done
  export JAVA_TOOL_OPTIONS="${JAVA_TOOL_OPTIONS:-} -Djavax.net.ssl.trustStore=$store/cacerts -Djavax.net.ssl.trustStorePassword=changeit"
fi
"$@"
