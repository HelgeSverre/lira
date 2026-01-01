# type_expressions

Type Expression Tests
Tests type checking (is) and type casting (as)
@expect-contains: x is int: true
@expect-contains: x is string: false
@expect-contains: s is string: true
@expect-contains: cast float to int: 3
@expect-contains: cast int to string: 123
@expect-contains: cast string to int: 456
