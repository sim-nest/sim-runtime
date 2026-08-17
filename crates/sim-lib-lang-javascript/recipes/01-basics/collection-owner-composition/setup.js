let sparse = [];
sparse[2] = "third";
let ordered = new Map();
ordered.set("first", 1);
ordered.set("second", 2);
[sparse.length, ...ordered.keys()];
