Object subclass: PositionableStream [
    | collection position limit |

    PositionableStream class >> on: aCollection [
        ^self basicNew initializeOn: aCollection
    ]

    initializeOn: aCollection [
        collection := aCollection.
        position := 0.
        limit := collection size.
        ^self
    ]

    position [
        ^position
    ]

    position: anInteger [
        anInteger isInteger ifFalse: [self error: 'Stream position must be an integer'].
        (anInteger < 0) ifTrue: [self error: 'Stream position out of bounds'].
        (anInteger > limit) ifTrue: [self error: 'Stream position out of bounds'].
        position := anInteger.
        ^position
    ]

    reset [
        position := 0.
        ^self
    ]

    atEnd [
        ^position >= limit
    ]

    readNext [
        self atEnd ifTrue: [^nil].
        position := position + 1.
        ^collection at: position
    ]

    peekNext [
        self atEnd ifTrue: [^nil].
        ^collection at: position + 1
    ]

    contents [
        | result |
        result := collection species new: limit.
        1 to: limit do: [:index |
            result streamIsResizable
                ifTrue: [result add: (collection at: index)]
                ifFalse: [result at: index put: (collection at: index)]
        ].
        ^result
    ]
]

PositionableStream subclass: ReadStream [
    next [
        ^self readNext
    ]

    peek [
        ^self peekNext
    ]
]

PositionableStream subclass: WriteStream [
    initializeOn: aCollection [
        collection := aCollection.
        position := 0.
        limit := 0.
        ^self
    ]

    grow [
        | size replacement |
        size := collection size * 2.
        size = 0 ifTrue: [size := 1].
        replacement := collection species new: size.
        1 to: limit do: [:index |
            replacement at: index put: (collection at: index)
        ].
        collection := replacement
    ]

    nextPut: value [
        | next |
        next := position + 1.
        next > collection size ifTrue: [
            collection streamIsResizable ifTrue: [
                collection add: value.
                position := next.
                position > limit ifTrue: [limit := position].
                ^value
            ].
            self grow
        ].
        collection at: next put: value.
        position := next.
        position > limit ifTrue: [limit := position].
        ^value
    ]

    nextPutAll: aCollection [
        aCollection do: [:each | self nextPut: each].
        ^aCollection
    ]

]

WriteStream subclass: ReadWriteStream [
    initializeOn: aCollection [
        collection := aCollection.
        position := 0.
        limit := collection size.
        ^self
    ]

    next [
        ^self readNext
    ]

    peek [
        ^self peekNext
    ]
]

SequenceableCollection extend [
    writeStream [
        ^WriteStream on: self
    ]

    streamIsResizable [
        ^false
    ]
]

OrderedCollection extend [
    streamIsResizable [
        ^true
    ]
]
