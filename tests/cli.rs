// SPDX-License-Identifier: MIT
// Copyright (c) 2018 Benjamin Eidelman, https://twitter.com/beneidel

//! The `jsondiffpatch` command against responses recorded from the reference command.

use std::io::Write;
use std::process::{Command, Stdio};

/// Request lines and the response line the reference command writes for each.
const EXCHANGES: &[(&str, &str)] = &[
    (r#"{"op":"diff","left":{},"right":{}}"#, r#"{}"#),
    (
        r#"{"op":"diff","left":{"x":-2,"c":43,"tags":false,"name":69},"right":{"tags":"eiusmod sed sit","y":true,"x":-2,"c":43,"name":79}}"#,
        r#"{"delta":{"tags":[false,"eiusmod sed sit"],"name":[69,79],"y":[true]}}"#,
    ),
    (
        r#"{"op":"diff","left":{"x":"do","id":"nostrud jumps","alpha":36},"right":{"alpha":36,"id":"exercitation enim fox"}}"#,
        r#"{"delta":{"x":["do",0,0],"id":["nostrud jumps","exercitation enim fox"]}}"#,
    ),
    (
        r#"{"op":"patch","left":{"name":-4,"id":{"b":"elit"},"y":31},"delta":{"id":{"b":["elit",75],"name":[{"count":"consectetur","e":"fox elit tempor","d":true,"meta":6}]},"y":[31,0,0]}}"#,
        r#"{"result":{"name":-4,"id":{"b":75,"name":{"count":"consectetur","e":"fox elit tempor","d":true,"meta":6}}}}"#,
    ),
    (
        r#"{"op":"unpatch","right":{"id":"nostrud the lorem","beta":-15,"gamma":"minim","alpha":"lorem"},"delta":{"beta":[null,-15],"alpha":[49,"lorem"]}}"#,
        r#"{"result":{"id":"nostrud the lorem","beta":null,"gamma":"minim","alpha":49}}"#,
    ),
    (
        r#"{"op":"unpatch","right":{"alpha":-7,"beta":null},"delta":{"beta":[31,null]}}"#,
        r#"{"result":{"alpha":-7,"beta":31}}"#,
    ),
    (
        r#"{"op":"reverse","delta":{"tags":[72,false],"z":[4,0,0]}}"#,
        r#"{"delta":{"tags":[false,72],"z":[4]}}"#,
    ),
    (
        r#"{"op":"reverse","delta":{"count":[null,true]}}"#,
        r#"{"delta":{"count":[true,null]}}"#,
    ),
    (
        r#"{"op":"reverse","delta":{"alpha":[null,{"d":"quick ut tempor"}]}}"#,
        r#"{"delta":{"alpha":[{"d":"quick ut tempor"},null]}}"#,
    ),
    (
        r#"{"op":"diff","left":{"value":-19,"b":[21,15,8,4]},"right":{"value":-19,"b":[21,15,8,4,78,51,52]},"format":"jsonpatch"}"#,
        r#"{"jsonpatch":[{"op":"add","path":"/b/4","value":78},{"op":"add","path":"/b/5","value":51},{"op":"add","path":"/b/6","value":52}]}"#,
    ),
    (
        r#"{"op":"diff","left":{"k2":"the","nested":70,"list":44},"right":{"k2":"the!","nested":70,"list":44},"format":"jsonpatch"}"#,
        r#"{"jsonpatch":[{"op":"replace","path":"/k2","value":"the!"}]}"#,
    ),
    (
        r#"{"op":"diff","left":{"user":"dog","alpha":81,"e":{"b":-10,"beta":14,"flag":"ut"}},"right":{"user":"dog","e":{"b":-10,"beta":14,"flag":"ut"},"id":66,"alpha":null},"format":"jsonpatch"}"#,
        r#"{"jsonpatch":[{"op":"replace","path":"/alpha","value":null},{"op":"add","path":"/id","value":66}]}"#,
    ),
    (
        r#"{"op":"diff","left":[68,71,14],"right":[68,71,14,114]}"#,
        r#"{"delta":{"3":[114],"_t":"a"}}"#,
    ),
    (
        r#"{"op":"diff","left":[{"v":85,"n":"quick"}],"right":[{"v":86,"n":"quick"}]}"#,
        r#"{"delta":{"0":{"v":[85,86]},"_t":"a"}}"#,
    ),
    (
        r#"{"op":"patch","left":[{"v":5,"n":"minim"},{"v":80,"n":"minim"},{"v":39,"n":"fox"}],"delta":{"0":{"v":[5,6]},"2":{"v":[39,40]},"_t":"a"}}"#,
        r#"{"result":[{"v":6,"n":"minim"},{"v":80,"n":"minim"},{"v":40,"n":"fox"}]}"#,
    ),
    (
        r#"{"op":"diff","left":{"meta":null,"count":"the jumps"},"right":{"count":"the jumps","meta":null},"options":{"textDiff":null}}"#,
        r#"{"error":{"name":"RequestError","message":"textDiff must be an object"}}"#,
    ),
    (
        r#"null"#,
        r#"{"error":{"name":"RequestError","message":"a request must be a JSON object"}}"#,
    ),
    (
        r#"{"op":"reverse","left":{"c":"fox sed quick","id":"fox"},"right":{"c":"fox sed quick","id":null},"delta":{"a":[1,2]},"format":"Delta"}"#,
        r#"{"error":{"name":"RequestError","message":"unknown format: \"Delta\""}}"#,
    ),
    (
        r#"{"op":"unpatch","right":[9,43,6,30,52,47,58,12,22,19,46,51,53],"delta":{"_t":"a","_4":["",2,3],"_8":["",6,3]}}"#,
        r#"{"result":[9,43,30,52,6,47,12,22,58,19,46,51,53]}"#,
    ),
    (
        r#"{"op":"diff","left":[51,16,45],"right":[45,51,"brown veniam brown",false,16],"options":{"arrays":{"detectMove":false}}}"#,
        r#"{"delta":{"0":[45],"2":["brown veniam brown"],"3":[false],"_t":"a","_2":[45,0,0]}}"#,
    ),
    (
        r#"{"op":"unpatch","right":[11,13,1,52,5,40,32,7,41],"delta":{"_t":"a","_5":["",8,3],"_8":["",1,3]}}"#,
        r#"{"result":[11,1,52,5,40,41,32,7,13]}"#,
    ),
    (
        r#"{"op":"diff","left":[{"id":4,"name":"over"},{"id":11,"name":"the"},{"id":36,"v":2},null],"right":[{"id":11,"name":"the","v":1}],"options":{"objectHash":"length"}}"#,
        r#"{"error":{"name":"TypeError","message":"Cannot read properties of null (reading 'length')"}}"#,
    ),
    (
        r#"{"op":"diff","left":[[6,2],null],"right":[[6,2]],"options":{"objectHash":"id","arrays":{"includeValueOnMove":true}}}"#,
        r#"{"error":{"name":"TypeError","message":"Cannot read properties of null (reading 'id')"}}"#,
    ),
    (
        r#"{"op":"diff","left":[{"id":34,"v":3}],"right":[{"id":34,"v":3}],"options":{"objectHash":"id"}}"#,
        r#"{}"#,
    ),
    (
        r#"{"op":"diff","left":{"text":"naïve quick brown jumps テキスト ipsum, over مرحبا enim minim ipsum ℕ lorem 🇫🇷","n":1},"right":{"text":"naïve tempor quick brown テキスト ipsum, ℕé ipsum minim enim مرح\nبا lorem 中文字","n":1}}"#,
        r#"{"delta":{"text":["@@ -1,14 +1,21 @@\n na%C3%AFve \n+tempor \n quick br\n@@ -22,14 +22,8 @@\n own \n-jumps \n %E3%83%86%E3%82%AD%E3%82%B9%E3%83%88\n@@ -34,44 +34,40 @@\n um, \n-over %D9%85%D8%B1%D8%AD%D8%A8%D8%A7 enim minim ipsum %E2%84%95\n+%E2%84%95%C3%A9 ipsum minim enim %D9%85%D8%B1%D8%AD%0A%D8%A8%D8%A7\n  lorem \n-%F0%9F%87%AB%F0%9F%87%B7\n+%E4%B8%AD%E6%96%87%E5%AD%97\n",0,2]}}"#,
    ),
    (
        r#"{"op":"diff","left":{"text":"aliqua ad\naliqua incididunt adipiscing fox; quis brown quick eiusmod ut\nveniam","n":1},"right":{"text":"aliqua ad\naliqua incididunt adipiscing fox; quis quick brown eiusmod ut\nveniam","n":1}}"#,
        r#"{"delta":{"text":["@@ -46,19 +46,19 @@\n uis \n-brown quick\n+quick brown\n  eiu\n",0,2]}}"#,
    ),
    (
        r#"{"op":"diff","left":"Ⅻ ℕ €100 the 𝔘𝔫𝔦 €100 amet 😀 dolor €100 tempor Ωmega مرحبا €100 lorem","right":"Ⅻ ℕ 𝔘𝔫𝔦 the 𝔘𝔫𝔦 €100 amet 😀 dolor €100 tempor tempor Ωmega مرحبا €100😀 lorem"}"#,
        r#"{"error":{"name":"URIError","message":"URI malformed"}}"#,
    ),
    (
        r#"{"op":"reverse","delta":["@@ -86,16 +86,20 @@\n own sed \n+the \n consecte\n@@ -183,16 +183,20 @@\n nostrud \n+sed \n na%C3%AFve no\n@@ -368,19 +368,13 @@\n  aliqua \n-na%C3%AFve \n brown\n",0,2]}"#,
        r#"{"delta":["@@ -86,20 +86,16 @@\n own sed \n-the \n consecte\n@@ -183,20 +183,16 @@\n nostrud \n-sed \n na%C3%AFve no\n@@ -368,13 +368,19 @@\n  aliqua \n+na%C3%AFve \n brown\n",0,2]}"#,
    ),
    (
        r#"{"op":"reverse","left":"abc","delta":["@@ -1,5 +1,5 @@\n-a%25b\n+a%2525b\n",0,2]}"#,
        r#"{"delta":["@@ -1,5 +1,5 @@\n-a%2525b\n+a%25b\n",0,2]}"#,
    ),
    (
        r#"{"op":"patch","left":{"a":1},"delta":["@@ -286,8 +286,27 @@\n am lorem\n+na%C3%AFve do enim minim\n",0,2]}"#,
        r#"{"result":"[object Object]"}"#,
    ),
    (
        r#"{"op":"format","delta":{"delta_":[false,0,0],"tags":[[true,"%",20,true]],"alpha":[{"Ab":"<script>"}]},"format":"console"}"#,
        r#"{"text":"{\n  alpha: {\n    \"Ab\": \"<script>\"\n  }\n  delta_: false\n  tags: [\n    true,\n    \"%\",\n    20,\n    true\n  ]\n}"}"#,
    ),
    (
        r#"{"op":"format","delta":{"_t":"a","_3":["",1.5,3],"_0":["",0,3]},"left":"abc","format":"console"}"#,
        r#"{"text":"[\n  0: ==> 0\n  1: \"b\"\n  2: \"c\"\n  3: ==> 1.5\n]"}"#,
    ),
    (
        r#"{"op":"diff","left":[-8,"veniam","Ωmega dog",77,-10,123456.789],"right":[-8,"veniam","Ωmega dog",-10,77],"format":"console"}"#,
        r#"{"text":"[\n  0: -8\n  1: \"veniam\"\n  2: \"Ωmega dog\"\n  3: 77\n  4: ==> 3\n  5: 123456.789\n]"}"#,
    ),
    (
        r#"{"op":"reverse","delta":{"k1":[true],"delta_":[true]},"format":"html"}"#,
        r#"{"text":"<div class=\"jsondiffpatch-delta jsondiffpatch-node jsondiffpatch-child-node-type-object\"><ul class=\"jsondiffpatch-node jsondiffpatch-node-type-object\"><li class=\"jsondiffpatch-deleted\" data-key=\"delta_\"><div class=\"jsondiffpatch-property-name\">delta_</div><div class=\"jsondiffpatch-value\"><pre>true</pre></div></li><li class=\"jsondiffpatch-deleted\" data-key=\"k1\"><div class=\"jsondiffpatch-property-name\">k1</div><div class=\"jsondiffpatch-value\"><pre>true</pre></div></li></ul></div>"}"#,
    ),
    (
        r#"{"op":"unpatch","right":{"e":{"tags":false,"y":44,"k1":33,"value":33,"<b>":null},"id":{"name":9,"tab\t":false,"nested":6,"\"q\"":false,"10":"jumps"},"nested":38},"delta":{"nested":[61,38]}}"#,
        r#"{"result":{"e":{"tags":false,"y":44,"k1":33,"value":33,"<b>":null},"id":{"10":"jumps","name":9,"tab\t":false,"nested":6,"\"q\"":false},"nested":61}}"#,
    ),
    (
        r#"{"op":"unpatch","right":{"ÿ":"jumps","value":"amet","$ref":82,"nested":"quis the","items":"fox quick x"},"delta":{"value":["dog amet","amet"],"$ref":[65,82],"items":["fox quick","fox quick x"]}}"#,
        r#"{"result":{"ÿ":"jumps","value":"dog amet","$ref":65,"nested":"quis the","items":"fox quick"}}"#,
    ),
    (
        r#"{"op":"diff","left":{"title":"dolor nostrud et","_":null,"value":85},"right":{"title":"dolor nostrud et","_":null,"name":[false,null],"":{"x":55,"k1":true,"gamma":-11}},"format":"console"}"#,
        r#"{"text":"{\n  : {\n    \"x\": 55,\n    \"k1\": true,\n    \"gamma\": -11\n  }\n  _: null\n  name: [\n    false,\n    null\n  ]\n  title: \"dolor nostrud et\"\n  value: 85\n}"}"#,
    ),
    (
        r#"{"op":"diff","left":[null,false,["nostrud adipiscing ad",[null,"enim quis",1e-07,"ad quick aliqua"]],null],"right":[false,["nostrud adipiscing ad",-16,[null,"enim quis",1e-07,"ad quick aliqua"]],null],"format":"jsonpatch"}"#,
        r#"{"jsonpatch":[{"op":"remove","path":"/2"},{"op":"remove","path":"/0"},{"op":"add","path":"/1","value":["nostrud adipiscing ad",-16,[null,"enim quis",1e-7,"ad quick aliqua"]]}]}"#,
    ),
    (
        r#"{"op":"patch","left":[1e-06,255.0,3.14159,19,5e-324]}"#,
        r#"{"result":[0.000001,255,3.14159,19,5e-324]}"#,
    ),
    (
        r#"{"op":"patch","left":{"at":"1999-01-31T99:00:59.123-05:30","n":1},"delta":{"at":["1999-01-31T99:00:59.123-05:30","1999-12-45T24:00:07"]},"reviveDates":true}"#,
        r#"{"result":{"at":"1999-12-45T24:00:07","n":1}}"#,
    ),
    (
        r#"{"op":"diff","left":{"at":"0000-00-31T99:30:07.999 Z","x":1},"right":{"at":"0000-06-31T12:30:60.0000001","x":1},"reviveDates":true}"#,
        r#"{"delta":{"at":["0000-00-31T99:30:07.999 Z","0000-06-31T12:30:60.0000001"]}}"#,
    ),
    (
        r#"{"op":"diff","left":{"at":"0042-06-31T24:60:00.1-05:30"},"right":{"at":"0042-06-01T99:30:60.123456 Z"},"reviveDates":true,"format":"html"}"#,
        r#"{"text":"<div class=\"jsondiffpatch-delta jsondiffpatch-node jsondiffpatch-child-node-type-object\"><ul class=\"jsondiffpatch-node jsondiffpatch-node-type-object\"><li class=\"jsondiffpatch-modified\" data-key=\"at\"><div class=\"jsondiffpatch-property-name\">at</div><div class=\"jsondiffpatch-value jsondiffpatch-left-value\"><pre>&quot;1942-07-02T01:00:00.001Z&quot;</pre></div><div class=\"jsondiffpatch-value jsondiffpatch-right-value\"><pre>&quot;0042-06-01T99:30:60.123456 Z&quot;</pre></div></li></ul></div>"}"#,
    ),
    (
        r#"{"op":"diff","left":{"a":null,"c":-20},"right":{"c":["the over"],"a":null},"options":{"cloneDiffValues":true},"format":"jsonpatch"}"#,
        r#"{"jsonpatch":[{"op":"replace","path":"/c","value":["the over"]}]}"#,
    ),
    (
        r#"{"op":"diff","left":{"beta":true},"right":{},"options":{"objectHash":"id"},"format":"html"}"#,
        r#"{"text":"<div class=\"jsondiffpatch-delta jsondiffpatch-node jsondiffpatch-child-node-type-object\"><ul class=\"jsondiffpatch-node jsondiffpatch-node-type-object\"><li class=\"jsondiffpatch-deleted\" data-key=\"beta\"><div class=\"jsondiffpatch-property-name\">beta</div><div class=\"jsondiffpatch-value\"><pre>true</pre></div></li></ul></div>"}"#,
    ),
    (
        r#"{"op":"diff","left":{"items":["テキスト naïve naïve",[true,[false,"naïve",-15,"dolor lorem"]],13,1.23e-18],"gamma":100.0},"right":{"beta":[true,null]},"options":{"cloneDiffValues":true}}"#,
        r#"{"delta":{"items":[["テキスト naïve naïve",[true,[false,"naïve",-15,"dolor lorem"]],13,1.23e-18],0,0],"gamma":[100,0,0],"beta":[[true,null]]}}"#,
    ),
    (
        r#"{"op":"diff","left":{"k1":"brown","e":{"x":92,"c":71,"title":-6},"d":null},"right":{"c":88,"count":{"z":80,"gamma":tre,"value":false},"meta":{},"k1":{}}}"#,
        r#"{"error":{"name":"SyntaxError","message":"Unexpected token 'e', ...\"\"gamma\":tre,\"value\":\"... is not valid JSON"}}"#,
    ),
    (
        r#"{"o"#,
        r#"{"error":{"name":"SyntaxError","message":"Unterminated string in JSON at position 3 (line 1 column 4)"}}"#,
    ),
    (
        r#"1.e5"#,
        r#"{"error":{"name":"SyntaxError","message":"Unterminated fractional number in JSON at position 2 (line 1 column 3)"}}"#,
    ),
];

fn run(args: &[&str], input: &str) -> (i32, String, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_jsondiffpatch"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the command starts");
    // A usage error exits without reading its input, so the write may meet a closed pipe.
    let _ = child.stdin.take().expect("stdin is piped").write_all(input.as_bytes());
    let output = child.wait_with_output().expect("the command finishes");
    (
        output.status.code().expect("the command exits"),
        String::from_utf8(output.stdout).expect("stdout is UTF-8"),
        String::from_utf8(output.stderr).expect("stderr is UTF-8"),
    )
}

#[test]
fn responds_like_the_reference_command() {
    for (request, response) in EXCHANGES {
        let (status, stdout, stderr) = run(&[], &format!("{request}\n"));
        assert_eq!(stdout, format!("{response}\n"), "request {request}");
        assert_eq!(
            status,
            i32::from(response.starts_with("{\"error\"")),
            "request {request}"
        );
        assert_eq!(stderr, "");
    }
}

#[test]
fn answers_every_line_in_order() {
    let input: String = EXCHANGES.iter().map(|(request, _)| format!("{request}\n\n")).collect();
    let expected: String = EXCHANGES.iter().map(|(_, response)| format!("{response}\n")).collect();
    let (status, stdout, _) = run(&[], &input);
    assert_eq!(stdout, expected);
    assert_eq!(status, 1);
}

#[test]
fn rejects_arguments_with_the_usage_text() {
    let (status, stdout, stderr) = run(&["--help"], "{}\n");
    assert_eq!(status, 2);
    assert_eq!(stdout, "");
    assert!(stderr.starts_with("usage: jsondiffpatch < requests.jsonl\n"));
}

#[test]
fn empty_input_writes_nothing() {
    assert_eq!(run(&[], ""), (0, String::new(), String::new()));
    assert_eq!(run(&[], "\n\n"), (0, String::new(), String::new()));
}
