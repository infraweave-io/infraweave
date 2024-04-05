FROM rust:slim-bullseye as builder

RUN mkdir /app
WORKDIR /app

COPY . /app/
COPY ./infrabridge_operator/Cargo.toml /app/infrabridge_operator/
COPY ./infrabridge_operator/Cargo.lock /app/infrabridge_operator/
COPY ./infrabridge_operator/src /app/infrabridge_operator/src/
COPY ./environments/ /app/environments/


RUN cd infrabridge_operator && cargo build --release

FROM rust:slim-bullseye

COPY --from=builder /app/infrabridge_operator/target/release/infrabridge_operator /usr/local/bin/infrabridge_operator

CMD ["infrabridge_operator"]