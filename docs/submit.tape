Output docs/submit.gif

Require canvas-cli

Set Shell fish
Set FontSize 32
Set Width 1500
Set Height 800
Set LoopOffset 50%
Set Framerate 24

Type "canvas-cli submit upload-test.pdf" Sleep 500ms  Enter
Sleep 6s

# select course
Left # fix rendering issue
Sleep 1s
Down@200ms 5
Sleep 1s
Up@200ms 5
Sleep 1s
Enter
Sleep 1s

# select assignment
Type@200ms "homework 17" Sleep 500ms  Enter

Sleep 8s
