// Two small jobs: copy the install command, and fire the ray in Fig. 1.

// --- copy -------------------------------------------------------------------

for (const button of document.querySelectorAll("[data-copy]")) {
  const source = document.getElementById(button.dataset.copy);
  button.addEventListener("click", async () => {
    const text = source.textContent.trim();
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // older browsers: select the text so ⌘C works
      const range = document.createRange();
      range.selectNodeContents(source);
      getSelection().removeAllRanges();
      getSelection().addRange(range);
      button.textContent = "Press ⌘C";
      return;
    }
    button.textContent = "Copied";
    clearTimeout(button.reset);
    button.reset = setTimeout(() => (button.textContent = "Copy"), 2000);
  });
}

// --- the ray ----------------------------------------------------------------

const beam = document.querySelector(".beam");
const target = document.querySelector(".target");
const again = document.querySelector(".fire");
const calm = matchMedia("(prefers-reduced-motion: reduce)");

const BEAM = 30;
const DUST = ["▓", "▒", "░", "·", " "];
const TARGETS = ["node_modules/", "DerivedData/", ".npm/_cacache", "Installer.dmg", "target/debug/", "old-logs/"];
let next = 0;
let firing = false;

const wait = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

async function fire() {
  if (firing || !beam) return;
  firing = true;
  const word = TARGETS[next++ % TARGETS.length];
  target.textContent = word;
  beam.textContent = "";
  await wait(500);

  // the beam crosses the gap
  for (let i = 1; i <= BEAM; i++) {
    beam.textContent = "━".repeat(i - 1) + (i === BEAM ? "✸" : "━");
    await wait(14);
  }
  // a flicker on impact
  for (let i = 0; i < 2; i++) {
    beam.classList.add("flash");
    await wait(45);
    beam.classList.remove("flash");
    await wait(45);
  }
  // the target turns to dust, one character at a time
  const stage = Array.from(word, () => 0);
  for (let frame = 0; frame < 10; frame++) {
    target.textContent = stage
      .map((s, i) => {
        if (frame > 0 && Math.random() < 0.4) stage[i] = Math.min(s + 1, DUST.length);
        return stage[i] === 0 ? word[i] : DUST[stage[i] - 1];
      })
      .join("");
    await wait(42);
  }
  target.textContent = "";
  // and the beam powers down
  for (let i = BEAM; i >= 0; i -= 3) {
    beam.textContent = " ".repeat(BEAM - i) + "━".repeat(Math.max(i - 1, 0)) + (i > 0 ? "✸" : "");
    await wait(16);
  }
  beam.textContent = "";
  firing = false;
}

if (beam && !calm.matches) {
  again.hidden = false;
  again.addEventListener("click", fire);
  fire();
}
