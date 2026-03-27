# Complete Example

This example shows the corpus compilation and query workflow using a small test corpus.

## Input data

The test corpus (`testcorp.vrt`) contains two documents with sentences in
vertical format. Each line has a word form and a lemma separated by a tab character:

```xml
<doc id="d1" date="2025-01">
<s>
The	the
cat	cat
sat	sit
on	on
the	the
mat	mat
.	.
</s>
<s>
A	a
dog	dog
chased	chase
the	the
cat	cat
.	.
</s>
</doc>
<doc id="d2" date="2025-02">
<s>
The	the
dog	dog
sat	sit
on	on
the	the
mat	mat
.	.
</s>
</doc>
```

The configuration file (`testcorp.conf`) defines the attributes and structures:

```
PATH "./data"
VERTICAL "./testcorp.vrt"
DEFAULTATTR word
ATTRIBUTE word
ATTRIBUTE lemma
ATTRIBUTE lc {
    DYNLIB internal
    DYNTYPE freq
    DYNAMIC utf8lowercase
    FROMATTR word
}
STRUCTURE s
STRUCTURE doc {
    ATTRIBUTE id
    ATTRIBUTE date
}
```

## Compile the corpus

```
$ encodevert -c ./testcorp.conf
```

This reads the vertical file and produces the binary corpus in `./data/`.

## Show corpus info

```
$ corpinfo -p ./testcorp.conf
./data/

$ corpinfo -p -s ./testcorp.conf
./data/
20
```

The corpus path and size (20 tokens).

## Build reverse indices

Reverse indices enable word lookups and concordancing:

```
$ mkrev ./testcorp.conf word
$ mkrev ./testcorp.conf doc.id
$ mkrev ./testcorp.conf doc.date
```

## Create dynamic attribute

The `lc` attribute is a lowercased version of `word`, computed on the fly:

```
$ mkdynattr ./testcorp.conf lc
```

## Compute frequency statistics

```
$ mkstats ./testcorp.conf word frq
```

## Token coverage

Count tokens covered by each structure attribute value:

```
$ mktokencov ./testcorp.conf
output prefix is ./data/
skipping token coverage calculation for structure s without attributes
calculating token coverage for doc
writing ./data/doc.id.token
writing ./data/doc.date.token
finished writing token coverage for doc
```

## Frequency list

List the most frequent words (using `-i 1` to include low-frequency items
in this small corpus):

```
$ lswl -i 1 ./testcorp.conf word
the	3
.	3
The	2
cat	2
sat	2
on	2
mat	2
dog	2
A	1
chased	1
```

## Concordance

Search for a word and display KWIC concordance lines:

```
$ conc ./testcorp.conf the
4	The cat sat on <the> mat . A dog chased the cat . The dog sat on the mat .
10	The cat sat on the mat . A dog chased <the> cat . The dog sat on the mat .
17	The cat sat on the mat . A dog chased the cat . The dog sat on <the> mat .
```
