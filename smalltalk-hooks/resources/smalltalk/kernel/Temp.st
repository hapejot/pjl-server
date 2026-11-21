Object subclass: Number [
    Number class >> readFrom: aStream radix: anInteger [
	"Answer the number read from the rest of aStream, converted to an
	 instance of the receiver. If the receiver is number, the class of the
	 result is undefined -- but the result is good.

         The exponent (for example 1.2e-1) is only parsed if anInteger is 10."

	<category: 'converting'>
        | c sgn int intexp frac exp isfloat |
        isfloat := false.
        sgn     := 1.
        int     := 0.
        intexp  := 1.
     
        c := aStream peek.
        c isNil ifTrue: [ ^0 ].
        c = $- ifTrue: [ sgn := -1. aStream next. ].
     
        c := aStream peek.
        c isNil ifTrue: [ ^0 ].
        c := c asUppercase.
        ((c isDigit: anInteger) or: [ c = $. ]) ifFalse: [ ^0 ].
     
        [ c notNil and: [
               c := c asUppercase.
               c isDigit: anInteger ] ] whileTrue: [
           aStream next.
           int := sgn * c digitValue + (int * anInteger).
           c := aStream peek
        ].
        c isNil ifTrue: [ ^int ].
     
        c = $. ifTrue: [
           aStream next.
           isfloat := true.
           [ c := aStream peek. c notNil and: [
                  c := c asUppercase.
                  c isDigit: anInteger ] ] whileTrue: [
              sgn := sgn / anInteger.
              int := sgn * c digitValue + int.
              aStream next
           ]
        ].
     
        exp := 0.
        (anInteger = 10 and: [c = $E]) ifFalse: [
             ^isfloat ifTrue: [ int asFloat ] ifFalse: [ int ] ].
     
        aStream next.
        c := aStream peek.
        c isNil ifTrue: [ ^int ].
        sgn := 1.
        c = $+ ifTrue: [ sgn :=  1. aStream next ].
        c = $- ifTrue: [ sgn := -1. aStream next ].
     
        [ c := aStream peek. c notNil and: [ c isDigit ] ] whileTrue: [
           exp := c digitValue + (exp * 10).
           aStream next
        ].
     
        int := int * (10 raisedToInteger: exp * sgn).
        ^int asFloat
    ]
]
