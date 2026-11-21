Object subclass: Dictionary [
    | keys values links buckets fallbackHead fallbackTail count |

    Dictionary class >> new [
        ^self new: 8
    ]

    Dictionary class >> new: capacity [
        ^self basicNew initialize: capacity
    ]

    initialize: capacity [
        keys := Array new: capacity.
        values := Array new: capacity.
        links := Array new: capacity.
        buckets := Array new: 8.
        fallbackHead := nil.
        fallbackTail := nil.
        count := 0.
        ^self
    ]

    size [
        ^count
    ]

    indexOfKey: key [
        | index hash found |
        hash := self bucketHashOf: key.
        (hash == nil) ifFalse: [
            found := nil.
            index := buckets at: ((hash bitAnd: buckets size - 1) + 1).
            [index == nil == false] whileTrue: [
                ((keys at: index) = key) ifTrue: [found := index].
                index := links at: index
            ].
            index := fallbackHead.
            [index == nil == false and: [found == nil or: [index < found]]] whileTrue: [
                ((keys at: index) = key) ifTrue: [^index].
                index := links at: index
            ].
            (found == nil) ifTrue: [^0].
            ^found
        ].
        index := 1.
        [index <= count] whileTrue: [
            ((keys at: index) = key) ifTrue: [^index].
            index := index + 1
        ].
        ^0
    ]

    bucketHashOf: key [
        <primitive: VMpr_Dictionary_bucketHash>
        self error: 'Dictionary key hashing failed'
    ]

    indexKey: index [
        | hash bucket |
        hash := self bucketHashOf: (keys at: index).
        (hash == nil) ifTrue: [
            (fallbackHead == nil)
                ifTrue: [fallbackHead := index]
                ifFalse: [links at: fallbackTail put: index].
            fallbackTail := index
        ] ifFalse: [
            bucket := (hash bitAnd: buckets size - 1) + 1.
            links at: index put: (buckets at: bucket).
            buckets at: bucket put: index
        ]
    ]

    rehash: capacity [
        buckets := Array new: capacity.
        links := Array new: keys size.
        fallbackHead := nil.
        fallbackTail := nil.
        1 to: count do: [:index | self indexKey: index]
    ]

    includesKey: key [
        ^(self indexOfKey: key) > 0
    ]

    at: key [
        ^self at: key ifAbsent: [self error: 'Dictionary key not found']
    ]

    at: key ifAbsent: aBlock [
        | index |
        index := self indexOfKey: key.
        index = 0 ifTrue: [^aBlock value].
        ^values at: index
    ]

    at: key put: value [
        | index |
        index := self indexOfKey: key.
        index = 0 ifTrue: [
            count = keys size ifTrue: [self grow].
            count = buckets size ifTrue: [self rehash: buckets size * 2].
            index := count + 1.
            keys at: index put: key.
            values at: index put: value.
            count := index.
            self indexKey: index.
            ^value
        ].
        values at: index put: value.
        ^value
    ]

    removeKey: key [
        ^self removeKey: key ifAbsent: [self error: 'Dictionary key not found']
    ]

    removeKey: key ifAbsent: aBlock [
        | index result |
        index := self indexOfKey: key.
        index = 0 ifTrue: [^aBlock value].
        result := values at: index.
        [index < count] whileTrue: [
            keys at: index put: (keys at: index + 1).
            values at: index put: (values at: index + 1).
            index := index + 1
        ].
        keys at: count put: nil.
        values at: count put: nil.
        count := count - 1.
        self rehash: buckets size.
        ^result
    ]

    keys [
        | result |
        result := Array new: count.
        1 to: count do: [:index | result at: index put: (keys at: index)].
        ^result
    ]

    values [
        | result |
        result := Array new: count.
        1 to: count do: [:index | result at: index put: (values at: index)].
        ^result
    ]

    grow [
        | capacity newKeys newValues newLinks |
        capacity := keys size * 2.
        capacity = 0 ifTrue: [capacity := 1].
        newKeys := Array new: capacity.
        newValues := Array new: capacity.
        newLinks := Array new: capacity.
        1 to: count do: [:index |
            newKeys at: index put: (keys at: index).
            newValues at: index put: (values at: index).
            newLinks at: index put: (links at: index)
        ].
        keys := newKeys.
        values := newValues.
        links := newLinks
    ]

    keysAndValuesDo: aBlock [
        1 to: count do: [:index |
            aBlock value: (keys at: index) value: (values at: index)
        ].
        ^self
    ]

    do: aBlock [
        self keysAndValuesDo: [:key :value | aBlock value: value].
        ^self
    ]
]
