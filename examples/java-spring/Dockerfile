# syntax=docker/dockerfile:1
#
# Build from the repo root:
#   docker build -f examples/java-spring/Dockerfile -t ssr-wasm-java-spring .
#   docker run --rm -p 8006:8006 ssr-wasm-java-spring
#
# Unit tests:            docker build -f examples/java-spring/Dockerfile --target test .
# Behind a TLS-inspecting proxy, pass its root CA for the Maven and libextism downloads:
#   docker build --secret id=ca,src=/path/to/proxy-ca.pem -f examples/java-spring/Dockerfile ...

ARG JAVA_VERSION=25
ARG UBUNTU_VERSION=noble
ARG EXTISM_VERSION=1.41.0

# ---- build: Maven (via the wrapper) on a JDK; also fetches libextism ----
FROM eclipse-temurin:${JAVA_VERSION}-jdk-${UBUNTU_VERSION} AS build
ARG JAVA_VERSION
WORKDIR /src
COPY examples/java-spring/docker/ /usr/local/share/build/

# libextism, the native library the Extism Java SDK calls through JNA. Checksums from the
# release's *.checksum.txt files.
ARG EXTISM_VERSION TARGETARCH
ARG EXTISM_SHA256_amd64=753cef478c853556212f6ad64b6e98adbc8b7908931c54b79678bb7c1fb2643b
ARG EXTISM_SHA256_arm64=c8bf58093b036a6741408511c74ba27da1d6f5c9a48b2668fa7638a7028ad66c
RUN --mount=type=secret,id=ca,required=false \
    set -eu; \
    case "$TARGETARCH" in amd64) arch=x86_64 sha=$EXTISM_SHA256_amd64 ;; arm64) arch=aarch64 sha=$EXTISM_SHA256_arm64 ;; \
      *) echo "unsupported arch $TARGETARCH" >&2; exit 1 ;; esac; \
    /usr/local/share/build/with-proxy-ca.sh java /usr/local/share/build/Fetch.java \
      "https://github.com/extism/extism/releases/download/v${EXTISM_VERSION}/libextism-${arch}-unknown-linux-gnu-v${EXTISM_VERSION}.tar.gz" \
      "$sha" /tmp/libextism.tar.gz; \
    mkdir -p /opt/extism/lib; \
    tar -xzf /tmp/libextism.tar.gz -C /opt/extism/lib libextism.so; \
    rm /tmp/libextism.tar.gz

# Maven and the dependencies first, in their own layer, so source changes don't re-download them.
COPY examples/java-spring/mvnw examples/java-spring/pom.xml ./
COPY examples/java-spring/.mvn/ .mvn/
RUN --mount=type=secret,id=ca,required=false \
    /usr/local/share/build/with-proxy-ca.sh ./mvnw -B -q dependency:go-offline

COPY examples/java-spring/src/ src/
RUN --mount=type=secret,id=ca,required=false \
    /usr/local/share/build/with-proxy-ca.sh ./mvnw -B -q -DskipTests package \
 && java -Djarmode=tools -jar target/app.jar extract --layers --destination /out/app

# A Java runtime with only the modules the app uses (jdeps finds them): ~half the size of the JRE.
RUN modules=$(jdeps --ignore-missing-deps --print-module-deps --multi-release ${JAVA_VERSION} --recursive \
      --class-path '/out/app/dependencies/lib/*' /out/app/application/app.jar) \
 && echo "jlink modules: $modules" \
 && jlink --add-modules "$modules" --strip-debug --no-man-pages --no-header-files --output /out/jre

# ---- test: only built with --target test ----
FROM build AS test
COPY packages/components/dist/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm
COPY packages/components/dist/loader-bundle/demo-components/ /app/stencil/
ENV SSR_WASM_PATH=/app/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil \
    LD_LIBRARY_PATH=/opt/extism/lib
RUN --mount=type=secret,id=ca,required=false \
    /usr/local/share/build/with-proxy-ca.sh ./mvnw -B test -DargLine=--enable-native-access=ALL-UNNAMED

# ---- runtime: Ubuntu (libextism needs glibc) + the jlinked runtime ----
FROM ubuntu:${UBUNTU_VERSION}
RUN useradd --system --uid 10001 --create-home --home-dir /home/app app
ENV JAVA_HOME=/opt/java \
    PATH=/opt/java/bin:$PATH \
    LANG=C.UTF-8
COPY --from=build /out/jre/ /opt/java/
WORKDIR /app
COPY --from=build /opt/extism/lib/libextism.so /opt/extism/lib/libextism.so
# The jar extracted into Spring Boot's layers (least to most frequently changed): app.jar plus
# lib/, run with plain `java -jar`, which is what the JVM's AOT cache needs.
COPY --from=build /out/app/dependencies/ ./
COPY --from=build /out/app/spring-boot-loader/ ./
COPY --from=build /out/app/snapshot-dependencies/ ./
COPY --from=build /out/app/application/ ./
COPY packages/components/dist/ssr-wasm/index.wasm /app/ssr-wasm/index.wasm
COPY packages/components/dist/loader-bundle/demo-components/ /app/stencil/
RUN install -d -o app -g app /app/aot
ENV PORT=8006 \
    SSR_WASM_PATH=/app/ssr-wasm/index.wasm \
    STENCIL_CLIENT_DIR=/app/stencil \
    LD_LIBRARY_PATH=/opt/extism/lib \
    JAVA_OPTS="-XX:MaxRAMPercentage=40 -XX:+UseSerialGC --enable-native-access=ALL-UNNAMED"
USER app

# A training run, as the runtime user, that starts the application context and exits. It fills
# two caches, so the container starts faster:
# - the JVM's AOT cache (JEP 483/514): Spring's and the app's classes, loaded and linked;
# - wasmtime's (~/.cache/wasmtime): creating the pool makes libextism compile the ~6.5 MB
#   module to machine code, which later instances load instead of recompiling. On a CPU with
#   different features that's simply a miss: the server compiles and refills it.
RUN java $JAVA_OPTS -XX:AOTCacheOutput=/app/aot/app.aot -Dspring.context.exit=onRefresh -jar app.jar

EXPOSE 8006
HEALTHCHECK --interval=10s --timeout=5s --start-period=60s \
  CMD bash -c 'exec 3<>/dev/tcp/127.0.0.1/8006 && printf "GET /health HTTP/1.0\r\n\r\n" >&3 && grep -q "^ok" <&3'
ENTRYPOINT ["sh", "-c", "exec java $JAVA_OPTS -XX:AOTCache=/app/aot/app.aot -jar app.jar \"$@\"", "--"]
