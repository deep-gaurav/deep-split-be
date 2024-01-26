ARG BINARY_NAME_DEFAULT=deepsplit_be

FROM clux/muslrust:stable as builder

# Create a non-root user
RUN groupadd -g 10001 -r dockergrp && useradd -r -g dockergrp -u 10001 dockeruser

ARG BINARY_NAME_DEFAULT
ENV BINARY_NAME=$BINARY_NAME_DEFAULT

# Build dummy main with the project's Cargo lock and toml
COPY Cargo.lock .
COPY Cargo.toml .
RUN mkdir src && echo "fn main() {print!(\"Dummy main\");} // dummy file" > src/main.rs
RUN set -x && cargo build --target x86_64-unknown-linux-musl --release
RUN ["/bin/bash", "-c", "set -x && rm target/x86_64-unknown-linux-musl/release/deps/${BINARY_NAME//-/_}*"]

# Now add the rest of the project and build the real main
COPY src ./src
COPY .env .
COPY deepsplit.sqlite .
RUN set -x && cargo build --target x86_64-unknown-linux-musl --release
RUN mkdir -p /build-out
RUN set -x && cp target/x86_64-unknown-linux-musl/release/$BINARY_NAME /build-out/

# Create a minimal docker image 
FROM scratch

# Copy user information from the builder stage
COPY --from=builder /etc/passwd /etc/passwd
USER dockeruser

ARG BINARY_NAME_DEFAULT
ENV BINARY_NAME=$BINARY_NAME_DEFAULT

ENV RUST_LOG="error,$BINARY_NAME=info"
COPY --from=builder /build-out/$BINARY_NAME /

# Start with an execution list (there is no sh in a scratch image)
# No shell => no variable expansion, |, <, >, etc 
# Hard coded start command
CMD ["/deepsplit_be"]
