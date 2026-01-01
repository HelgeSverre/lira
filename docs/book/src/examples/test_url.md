# test_url

URL Module Tests
@expect-contains: encode: hello+world
@expect-contains: decode: hello world
@expect-contains: parse host: example.com
@expect-contains: parse path: /path/to/resource
@expect-contains: parse port: 8080
@expect-contains: query get: bar
@expect-contains: roundtrip passed


