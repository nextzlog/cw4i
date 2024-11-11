CW4ISR
====

![image](https://img.shields.io/badge/Go-1.20-red.svg)
![image](https://img.shields.io/badge/license-MIT-darkblue.svg)
![badge](https://github.com/nextzlog/cw4i/actions/workflows/build.yaml/badge.svg)
![badge](https://github.com/nextzlog/cw4i/actions/workflows/clean.yaml/badge.svg)

CW4ISR is a highly programmable Morse Decoder that assists automated amateur radio operations.

## Get Started

Download the nightly build [here](https://github.com/nextzlog/cw4i/releases/tag/nightly).
Some unofficial releases may be available [here](https://github.com/jucky154/cw4i/releases).

### Windows

Just click `cw4i.exe` to start CW4ISR.

### macOS

You can install `cw4i.app` from `cw4i.dmg`.

## Build

If you have a Go development environment ready, you can install CW4ISR with the following command.

```sh
$ go install github.com/nextzlog/cw4i@HEAD
$ cw4i
```

Alternatively, you can build CW4ISR with the command.

```sh
$ git clone https://github.com/nextzlog/cw4i
$ cd cw4i
$ go mod tidy
$ go build
$ ./cw4i
```

## Documents

[GoDoc](https://pkg.go.dev/github.com/nextzlog/cw4i)

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
