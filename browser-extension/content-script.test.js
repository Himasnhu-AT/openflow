// node --test browser-extension/
const test = require("node:test");
const assert = require("node:assert");
const { validName, canon, nameFromLines, parseCaption, pickActive } = require("./content-script.js");

test("names", () => {
  assert.ok(validName("Priya Shah"));
  assert.ok(validName("José Álvarez"));
  assert.ok(!validName("You"));
  assert.ok(!validName("More options"));
  assert.ok(!validName("Priya Shah is presenting"));
  assert.ok(!validName("x"));
  assert.ok(!validName("meet.google.com/abc-defg-hij"));
  assert.ok(!validName("10:42"));
  assert.equal(canon("Priya Shah (You)"), "Priya Shah");
});

test("tile text", () => {
  assert.deepEqual(nameFromLines(["", "Priya Shah", "more_vert"]), { name: "Priya Shah", isSelf: false });
  assert.deepEqual(nameFromLines(["You", "Himanshu"]), { name: "Himanshu", isSelf: true });
  assert.deepEqual(nameFromLines(["mic_off"]).name, null);
});

test("captions", () => {
  assert.deepEqual(parseCaption(["Priya Shah", "So the launch moves", "to Thursday."]), {
    name: "Priya Shah",
    text: "So the launch moves to Thursday.",
  });
  assert.equal(parseCaption(["just some text"]), null);
  assert.equal(parseCaption(["More options", "x"]), null);
});

test("active speaker", () => {
  const counts = new Map([["a", 2], ["b", 7], ["me", 12]]);
  assert.equal(pickActive(counts, new Set(["me"]), 3), "b");
  assert.equal(pickActive(new Map([["a", 2]]), new Set(), 3), null);
});
