//! `--selftest`: the rule-layer checks of SelfTest.swift, same inputs and expected outputs.
//! The Segmenter and Meeting to-do checks of SelfTest.swift live with those modules (team B), not here.

use std::fmt::Debug;
use std::time::Instant;

use super::*;
use crate::whisper_filter::{filter_whisper, WhisperSegment};

/// Collects check results.
#[derive(Default)]
pub struct Checker {
    pub passed: usize,
    pub failures: Vec<String>,
}

impl Checker {
    pub fn check<T: PartialEq + Debug>(&mut self, name: &str, got: T, want: T) {
        if got == want {
            self.passed += 1;
        } else {
            self.failures.push(format!("FAIL {name}\n       got:  {got:?}\n       want: {want:?}"));
        }
    }
}

fn s(v: &[&str]) -> Vec<String> {
    v.iter().map(|x| x.to_string()).collect()
}

pub fn fillers(c: &mut Checker) {
    c.check("fillers 1", strip_fillers("Um, so, tomorrow is fine. See you then."), "so, tomorrow is fine. See you then.".into());
    c.check("fillers 2", strip_fillers("I um I really want to um go"), "I really want to go".into());
    c.check("stutter", strip_fillers("we we we ship on tuesday tomorrow, tomorrow we look"), "we ship on tuesday tomorrow we look".into());
    c.check("umbrella kept", strip_fillers("Hmm. The umbrella is there."), "The umbrella is there.".into());
    c.check("scratch start", strip_fillers("Hey so tomorrow, tomorrow is fine, actually scratch that, let's do Thursday instead."), "let's do Thursday instead.".into());
    c.check("scratch mid", strip_fillers("We agreed on Monday. Then um Tuesday scratch that Wednesday works"), "We agreed on Monday. Wednesday works".into());
    c.check("scratch closed sentence", strip_fillers("I told him she shouldn't be his girlfriend. She's a horrible person. Never mind. She's a great person, but they're not compatible."), "I told him she shouldn't be his girlfriend. She's a great person, but they're not compatible.".into());
    c.check("scratch nevermind one word", strip_fillers("send it monday nevermind send it friday"), "send it friday".into());
    c.check("restart clause", strip_fillers("Starting on a Thursday night, I was invited to a startup event. Because banking people are heaps more, sorry, because startup people are heaps more genuine than banking people, I might have indulged."), "Starting on a Thursday night, I was invited to a startup event. because startup people are heaps more genuine than banking people, I might have indulged.".into());
    c.check("restart sentence", strip_fillers("today's weather is very quiet sorry today's weather is very sunny"), "today's weather is very sunny".into());
    c.check("restart no wait", strip_fillers("send it to John no wait send it to Alex by Friday"), "send it to Alex by Friday".into());
    c.check("restart pronoun", strip_fillers("I want to go, sorry, I need to leave"), "I need to leave".into());
    c.check("apology kept", strip_fillers("sorry I'm late, the train was delayed"), "sorry I'm late, the train was delayed".into());
    c.check("apology kept 2", strip_fillers("thanks for the report, sorry I forgot to say the numbers look great"), "thanks for the report, sorry I forgot to say the numbers look great".into());
    c.check("repeat restart", strip_fillers("look mate i have a thesing what thief i have a few things i want to discuss"), "look mate i have a few things i want to discuss".into());
    c.check("article restart", strip_fillers("This is fair a fair bit of work"), "This is a fair bit of work".into());
    c.check("article restart kept", strip_fillers("by the time the time comes we know"), "by the time the time comes we know".into());
    c.check("repeat kept", strip_fillers("we ship on thursday and we ship on friday"), "we ship on thursday and we ship on friday".into());
    c.check("swap pronoun", strip_fillers("i caught a cold from my partner and yeah she actually he was also sick"), "i caught a cold from my partner and yeah he was also sick".into());
    c.check("swap day", strip_fillers("we ship on Tuesday, actually Thursday, and then rest"), "we ship on Thursday, and then rest".into());
    c.check("swap number", strip_fillers("bring 5 no 6 chairs"), "bring 6 chairs".into());
    c.check("swap not same class", strip_fillers("it's actually good and she actually likes it"), "it's actually good and she actually likes it".into());
    c.check("scratch none", strip_fillers("nothing to scratch here"), "nothing to scratch here".into());
}

pub fn commands(c: &mut Checker) {
    c.check("newline", apply_commands("first line new line second line, new paragraph third"), "first line\nsecond line\n\nthird".into());
    c.check("bullets", apply_commands("shopping list bullet point milk bullet point eggs, bullet point bread"), "shopping list\n- milk\n- eggs\n- bread".into());
    c.check("email", apply_commands("send it to sam at example dot com today"), "send it to sam@example.com today".into());
    c.check("url", apply_commands("go to acmehub dot com dot au"), "go to acmehub.com.au".into());
    c.check("question", apply_commands("are you coming question mark"), "are you coming?".into());
    c.check("full stop", apply_commands("that's it full stop"), "that's it.".into());
}

pub fn emoji(c: &mut Checker) {
    c.check("emoji laugh cry", apply_commands("that was so funny laugh cry emoji, anyway see you"), "that was so funny 😂 anyway see you".into());
    c.check("emoji crying laugh", apply_commands("haha crying laugh emoji"), "haha 😂".into());
    c.check("emoji article", apply_commands("send me a thumbs up emoji when done"), "send me 👍 when done".into());
    c.check("emoji plural", apply_commands("fire emoji fire emoji"), "🔥 🔥".into());
    c.check("emoji hyphen", apply_commands("thumbs-up emoji"), "👍".into());
    c.check("emoji none", apply_commands("the emoji keyboard is open"), "the emoji keyboard is open".into());
}

pub fn guardrail(c: &mut Checker) {
    let raw = "okay so the plan is we ship on thursday and then we do the reel for instagram";
    c.check("guard ok", looks_unfaithful(raw, "Okay, so the plan is we ship on Thursday and then we do the reel for Instagram.", &[]), false);
    c.check("guard chatty", looks_unfaithful(raw, "Sure! Here is a cleaned up version of your message about shipping.", &[]), true);
    c.check("guard short", looks_unfaithful(raw, "Ship Thursday.", &[]), true);
    c.check("guard dict", looks_unfaithful("send the acme hub report to alex by friday please thanks", "send the Acme Hub report to alex by friday please thanks", &s(&["Acme Hub"])), false);
}

pub fn lists(c: &mut Checker) {
    c.check("list ordinals", apply_lists("i have a few things i want to discuss. firstly, what is it that we're aiming for? secondly, what timeline we're targeting? thirdly, where are we going to execute this?"), "i have a few things i want to discuss.\n- what is it that we're aiming for?\n- what timeline we're targeting?\n- where are we going to execute this?".into());
    c.check("list numbers", apply_lists("Three things: number one the pricing slide, number two the roadmap."), "Three things:\n- the pricing slide.\n- the roadmap.".into());
    c.check("list single marker", apply_lists("firstly, thanks for coming"), "firstly, thanks for coming".into());
    c.check("list first adjective", apply_lists("the first time was fine and the second time too"), "the first time was fine and the second time too".into());
    c.check("counted list", apply_counted_lists("hey Sam, there are three things Wispr Flow does really well: it's insanely fast, it's insanely accurate, it gets names right."), "hey Sam, there are three things Wispr Flow does really well:\n1. it's insanely fast\n2. it's insanely accurate\n3. it gets names right".into());
    c.check("counted list after full stop", apply_counted_lists("hey Sam, there are three things a free solution also does really well. it's insanely fast, it's insanely accurate, and it gets names right."), "hey Sam, there are three things a free solution also does really well:\n1. it's insanely fast\n2. it's insanely accurate\n3. it gets names right".into());
    c.check("counted list with and", apply_counted_lists("Two reasons: the price is right, and the timing works. Let me know."), "Two reasons:\n1. the price is right\n2. the timing works\nLet me know.".into());
    c.check("counted list mismatch", apply_counted_lists("three things: fast and accurate."), "three things: fast and accurate.".into());
}

pub fn numbers(c: &mut Checker) {
    c.check("num billion dollars", apply_numbers("whisperflow is valued at two billion dollars and granola around one point five billion."), "whisperflow is valued at $2 billion and granola around 1.5 billion.".into());
    c.check("num dollars a month", apply_numbers("it's twenty dollars a month"), "it's $20 a month".into());
    c.check("num percent", apply_numbers("about fifteen percent of users"), "about 15% of users".into());
    c.check("num small stays", apply_numbers("i have two things and three ideas"), "i have two things and three ideas".into());
    c.check("num twenty five", apply_numbers("twenty five people came"), "25 people came".into());
    c.check("num hundred and fifty", apply_numbers("a hundred and fifty dollars"), "$150".into());
    c.check("num thousands", apply_numbers("about twenty thousand people"), "about 20,000 people".into());
    c.check("num digits unit", apply_numbers("it costs 20 dollars and 2 billion dollars overall"), "it costs $20 and $2 billion overall".into());
    c.check("num list marker untouched", apply_numbers("number one the pricing"), "number one the pricing".into());
    c.check("num year", apply_numbers("in two thousand and twenty six"), "in 2026".into());
    c.check("num one point five million", apply_numbers("one point five million bucks"), "$1.5 million".into());
    c.check("num via commands", apply_commands("it's twenty dollars a month"), "it's $20 a month".into());
    c.check("num via fillers", strip_fillers(&apply_commands("it's twenty dollars a month")), "it's $20 a month".into());
}

pub fn dictionary(c: &mut Checker) {
    let d = s(&["Zentro: zentrow, zen tro", "Wispr Flow: whisper flow, whisperflow", "Demolisher: the molisha", "JezFlow"]);
    c.check("dict alias", apply_dictionary("i've so far done zentrow, but also i'm doing whisper flow at the moment.", &d), "i've so far done Zentro, but also i'm doing Wispr Flow at the moment.".into());
    c.check("dict multiword alias", apply_dictionary("people call me the startup the molisha because", &d), "people call me the startup Demolisher because".into());
    c.check("dict term case", apply_dictionary("jezflow is great and zentro too", &d), "JezFlow is great and Zentro too".into());
    c.check("dict no partial", apply_dictionary("the zentrowle shop", &d), "the zentrowle shop".into());
    c.check("dict terms", dictionary_terms(&d), s(&["Zentro", "Wispr Flow", "Demolisher", "JezFlow"]));
}

pub fn learning(c: &mut Checker) {
    c.check("context names", context_names("Re: Zentro launch — Mail  Hi Alex, about the Wispr Flow reel. The plan is fine and Acme Hub too.", CONTEXT_NAMES_LIMIT), s(&["Zentro", "Alex", "Wispr Flow", "Acme Hub"]));
    c.check("context names skip common", context_names("Thanks Monday Hi The Window", CONTEXT_NAMES_LIMIT), vec![]);
    let pairs = |l: Vec<(String, String)>| l.into_iter().map(|(t, a)| format!("{t}<-{a}")).collect::<Vec<_>>();
    let l = learn_names("i've so far done zentrow, but also i'm doing whisper flow at the moment", "i've so far done Zentro, but also i'm doing Wispr Flow at the moment");
    c.check("learn names", pairs(l), s(&["Zentro<-zentrow", "Wispr Flow<-whisper flow"]));
    let l2 = learn_names("people call me the startup the molisha because", "people call me the startup Demolisher because");
    c.check("learn multiword alias", pairs(l2), s(&["Demolisher<-the molisha"]));
    let l3 = learn_names("we ship on tuesday", "We ship on Thursday.");
    c.check("learn ignores plain edits", l3.len(), 0);
    c.check("merge dictionary", merge_dictionary(&s(&["Zentro: zentrow", "JezFlow"]), &[("Zentro".into(), "zen tro".into()), ("Wispr Flow".into(), "whisper flow".into())]), s(&["Zentro: zentrow, zen tro", "JezFlow", "Wispr Flow: whisper flow"]));
    c.check("learn signal", learn_signals("i'll do Vegemite toast i know kfc for lunch", "i'll do kfc for lunch"), s(&["i know"]));
    c.check("learn signal none", learn_signals("we ship on tuesday", "We ship on Thursday."), vec![]);
}

pub fn meetings(c: &mut Checker) {
    c.check("meeting line", meeting_line("um just want to check in on your progress on no progress that's fine i'm just going to keep building on zentrow and show you guys on friday then", &s(&["Zentro: zentrow"])), "Just want to check in on your progress on no progress that's fine I'm just going to keep building on Zentro and show you guys on Friday then.".into());
    c.check("meeting line filler only", meeting_line("um", &[]), String::new());
    c.check("meeting line question", meeting_line("how's it been", &[]), "How's it been?".into());
    c.check("looks english", looks_english("good morning guys how's it been yeah it's been good"), true);
    c.check("looks not english", looks_english("啊點解佢咁唔識做"), false);
    c.check("chinese traditional", chinese_script("啊点解佢咁唔识做", true), "啊點解佢咁唔識做".into());
    c.check("chinese untouched", chinese_script("hello there", true), "hello there".into());
}

pub fn whisper(c: &mut Checker) {
    let seg = |text: &str, no_speech: f64, logprob: f64| WhisperSegment {
        text: text.into(),
        no_speech_prob: no_speech,
        avg_logprob: logprob,
        compression_ratio: 1.0,
    };
    c.check("whisper ghost", filter_whisper("Thank you.", &[], 3.0), String::new());
    c.check("whisper low confidence", filter_whisper("x", &[seg(" I'm a doctor.", 0.8, -0.9), seg(" Watch out for my training.", 0.0, -1.5), seg(" See you Thursday.", 0.05, -0.3)], 4.0), "See you Thursday.".into());
    c.check("whisper rate", filter_whisper("one two three four five six seven eight nine ten eleven twelve thirteen fourteen", &[], 2.0), String::new());
}

pub fn layout(c: &mut Checker) {
    c.check("supercasual rules", casual("Sounds good, see you at eight. Bring the charger.", &[], true), "sounds good see you at eight. bring the charger".into());
    c.check("casual keeps final stop", casual("Sounds good, see you at eight.", &[], false), "sounds good, see you at eight.".into());
    c.check("message layout no comma", layout_message("dear robin no worries at all i can deal with it this weekend kind regards jess"), "dear robin\n\nno worries at all i can deal with it this weekend\n\nkind regards\njess".into());
    c.check("casual rules", casual("Haha. No, I really don't want to do it, but can you do it?", &s(&["Acme Hub"]), false), "haha no, i really don't want to do it, but can you do it?".into());
    c.check("casual names kept", casual("See you at acme hub, Alex! I'll bring Instagram stuff.", &s(&["Acme Hub"]), false), "see you at Acme Hub, Alex i'll bring Instagram stuff.".into());
    c.check("casual two sentences", casual("Sounds good. See you at eight.", &[], false), "sounds good. see you at eight.".into());
    c.check("greeting line", layout_greeting("Hi Jordan, thanks for reaching out. I'll pass this time.\n\nKind regards\nTaylor"), "Hi Jordan,\n\nThanks for reaching out. I'll pass this time.\n\nKind regards\nTaylor".into());
    c.check("greeting line casual", layout_greeting("dear robin, no worries at all.\n\nkind regards\njess"), "dear robin,\n\nno worries at all.\n\nkind regards\njess".into());
    c.check("greeting line no comma", layout_greeting("dear robin no worries at all i can deal with it\n\nkind regards\njess"), "dear robin\n\nno worries at all i can deal with it\n\nkind regards\njess".into());
    c.check("greeting chat untouched", layout_greeting("hey jordan, thanks for reaching out"), "hey jordan, thanks for reaching out".into());
    c.check("question added", question_marks("hey alex, quick question. are you guys able to provide a free account or some credits"), "hey alex, quick question. are you guys able to provide a free account or some credits?".into());
    c.check("question replaces stop", question_marks("Can you send it by Friday. Thanks."), "Can you send it by Friday? Thanks.".into());
    c.check("question kept", question_marks("What we need is time. What is the plan?"), "What we need is time. What is the plan?".into());
    c.check("question list", question_marks("- what is it that we're aiming for\n- where are we going"), "- what is it that we're aiming for?\n- where are we going?".into());
    c.check("message layout casual", layout_message("dear robin, no worries at all, i can deal with it this weekend. kind regards, jess"), "dear robin,\n\nno worries at all, i can deal with it this weekend.\n\nkind regards,\njess".into());
    c.check("message layout no name", layout_message("Hi Alex, thanks for the deck. Cheers"), "Hi Alex,\n\nthanks for the deck.\n\nCheers".into());
    c.check("message layout chat untouched", layout_message("no worries, cheers"), "no worries, cheers".into());
    c.check("message layout thanks mid", layout_message("hey, thanks for that"), "hey, thanks for that".into());
    c.check("echo stripped", strip_echo("thanks this is working now but a few extra problems. as seen above we are missing paragraphs", "Tthanks, this is working now, but a few extra problems"), "as seen above we are missing paragraphs".into());
    c.check("echo kept", strip_echo("as seen above we are missing paragraphs", "thanks this is working now"), "as seen above we are missing paragraphs".into());
}

pub fn wer(c: &mut Checker) {
    c.check("wer same", word_error_rate("Hello, world!", "hello world"), 0.0);
    c.check("wer half", word_error_rate("we ship on thursday", "we ship on tuesday tomorrow"), 0.5);
    c.check("wer empty", word_error_rate("", ""), 0.0);
}

/// Every area, in one run.
pub fn run_all(c: &mut Checker) {
    fillers(c);
    commands(c);
    emoji(c);
    guardrail(c);
    lists(c);
    numbers(c);
    dictionary(c);
    learning(c);
    meetings(c);
    whisper(c);
    layout(c);
    wer(c);
}

/// Runs every rule check. Returns the number passed and a description of each failure.
pub fn self_test() -> (usize, Vec<String>) {
    let mut c = Checker::default();
    run_all(&mut c);
    (c.passed, c.failures)
}

/// How much the rule layer costs per dictation (all pre- and post-model rules on a 60-word transcript), in ms.
pub fn rules_benchmark_ms() -> f64 {
    let sample = "um so look mate i really don't want to trouble you that much but i have a thesing what thief i have a few things i want to discuss firstly what is it that we're aiming for sorry what are we aiming for secondly what timeline we're targeting thirdly where are we going to execute this and sam at example dot com new line cheers";
    let t0 = Instant::now();
    for _ in 0..200 {
        std::hint::black_box(question_marks(&apply_lists(&layout_greeting(&strip_fillers(&apply_commands(sample))))));
    }
    t0.elapsed().as_secs_f64() * 1000.0 / 200.0
}
