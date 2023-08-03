CW4ISR
====

![image](https://img.shields.io/badge/Go-1.20-red.svg)
![image](https://img.shields.io/badge/license-MIT-darkblue.svg)
![badge](https://github.com/nextzlog/cw4i/actions/workflows/build.yaml/badge.svg)

CW4ISR is a highly programmable Morse Decoder that assists automated amateur radio operations.

## Install

```sh
$ go install github.com/nextzlog/cw4i@HEAD
$ cw4i
```

## Event Handler

Place `cw4i.js` as follows to customize `core.Decoder` with JavaScript (ES5).

```js:cw4i.js
decoder.Program = function(message) {
  message.Text = message.Text.replace('5NN', '599');
  message.Text = message.Text.replace('ENN', '599');
  return message;
}
```

The `Program` function is invoked each time the decoder updates a message, which is an instance of the `Message` structure.

## Build

```sh
$ go build
```

## Contribution

Feel free to make issues at [nextzlog/todo](https://github.com/nextzlog/todo).
Follow [@nextzlog](https://twitter.com/nextzlog) on Twitter.

## License

### Author

[無線部開発班](https://nextzlog.dev)

- JG1VPP
- JS2FVO

### Clauses

[MIT License](LICENSE)
