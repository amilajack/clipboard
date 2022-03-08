% clipboard(1)
% Amila Welihinda
% January 2022

# NAME

clipboard - a better cli clipboard

# Examples

**Copy to os clipbard**

cb package.json

**Pipe to os clipboard**

cat package.json | cb
echo 'Hello World!' | cb

**Read clipboard contents**

cb | vim -

**Search clipboard contents**

cb | grep hello
