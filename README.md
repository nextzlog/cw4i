CW4ISR
====

![image](https://img.shields.io/badge/Go-1.20-red.svg)
![image](https://img.shields.io/badge/license-MIT-darkblue.svg)
![badge](https://github.com/nextzlog/cw4i/actions/workflows/build.yaml/badge.svg)

CW4ISR is a highly programmable Morse Decoder that assists automated amateur radio operations.

## Install

```sh
$ go install github.com/nextzlog/CW4I@HEAD
$ cw4i
```

## Event Handler

CW4ISR can be customized with JavaScript (ES5).
Place `cw4i.js` as follows to define an event handler which accepts and returns `message`:

```js:cw4i.js
function handler(message) {
  message.Text = message.Text.replace('5NN', '599');
  message.Text = message.Text.replace('ENN', '599');
  return message;
}

handler;
```

The function will be invoked every time CW4ISR updates the messages.
Each message is an instance of `Message` structure, whose methods can also be called.

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
