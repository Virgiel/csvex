# CSV Explorer

`csvex` is a command line CSV explorer

## Usage

Run `csvex` by providing the CSV filename:

```
csvex <filename>
```

## Key bindings

### Normal

| Key            | Action                        |
| -------------- | ----------------------------- |
| `h` or `←`     | Move cursor to the left       |
| `k` or `→`     | Move cursor to the right      |
| `k` or `↑`     | Move cursor up                |
| `j` or `↓`     | Move cursor down              |
| `H` or `Maj ←` | Move current col to the left  |
| `K` or `Maj →` | Move current col to the right |
| `-`            | Hide current col              |
| `/`            | Switch to filter mode         |
| `s`            | Switch to size mode           |
| `g`            | Switch to navigation mode     |
| `r`            | Reload file content           |
| `q`            | Exit                          |

### Filter

| Key     | Action                |
| ------- | --------------------- |
| `Esc`   | Return to normal mode |
| `Tab`   | Toggle col index view |
| `Enter` | Apply filter          |
| other   | Write into prompt     |

### Size

| Key        | Action                          |
| ---------- | ------------------------------- |
| `Esc`      | Return to normal mode           |
| `h` or `←` | Reduce col size by one          |
| `k` or `→` | Augment col size by one         |
| `k` or `↑` | Match col size with its content |
| `j` or `↓` | Constrain col size              |
| `r`        | Reset all cols size             |
| `f`        | Fit all cols to their content   |

### Size

#### Empty prompt

| Key              | Action                |
| ---------------- | --------------------- |
| `Esc` or `Enter` | Return to normal mode |
| `h` or `←`       | Move to first col     |
| `k` or `→`       | Move to last col      |
| `k` or `↑`       | Move to first row     |
| `j` or `↓`       | Move to last row      |
| `r`              | Reset all cols size   |
| other            | Write into prompt     |

#### With prompt

| Key     | Action                                 |
| ------- | -------------------------------------- |
| `Esc`   | Return to normal mode and reset cursor |
| `Enter` | Return to normal mode and keep cursor  |
| other   | Write into prompt                      |

## Filter syntax

Filters are logical expressions that are used to filter displayed rows.

### Column index

Columns are referenced using their indexes. To require the presence of a column
you can simply use its index:

```
4
```

### Comparison operators

| Operator     | Meaning          |
| ------------ | ---------------- |
| `eq` or `==` | Equal            |
| `nq` or `!=` | Not Equal        |
| `gt` or `>`  | Greater          |
| `lt` or `<`  | Less             |
| `ge` or `>=` | Greater or Equal |
| `le` or `<=` | Less or Equal    |

Comparisons are made between a column and a value. A value can be a string or a
number :

```
1 == Chocolate
2 != "I love chocolate"
4 >= -49.3
```

If the value is a string, a string comparison is performed. If the value is a
number, the column's content is converted to a number to perform a number
comparison, if the conversion failed a string comparison is performed.

### Regex matching

| Operator         | Meaning |
| ---------------- | ------- |
| `matches` or '~' | Matches |

Matchings are made between a column and a regular expression :

```
1 ~ [0-6]
3 ~ "I love [1-9] kinds of chocolate"
```

### Logical operators

| Operator       | Meaning |
| -------------- | ------- |
| `not` or `!`   | Not     |
| `and` or `&&`  | And     |
| `or` or `\|\|` | Or      |

Multiple operations can be composed using logical operators :

```
(1 && 3) || (!2 && 4 == Chocolate)
```

### Slice operator

| Operator | Meaning        |
| -------- | -------------- |
| `[i:j]`  | `[i,i+length]` |
| `[i-j]`  | `[i,j]`        |
| `[i]`    | `[i]`          |
| `[:j]`   | `[0,length]`   |
| `[i:]`   | `[i,end]`      |

You can take a slice of a column's content before performing an operation :

```
43[3:4] == Choco
```
