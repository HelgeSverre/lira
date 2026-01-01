# test_base64

Base64 Encoding Tests
@expect-contains: encode hello: SGVsbG8gV29ybGQh
@expect-contains: decode: Hello World!
@expect-contains: roundtrip passed
@expect-contains: url encode passed
