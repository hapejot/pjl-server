ArrayedCollection extend [
    select: aBlock [
        | selected count result |
        selected := self species new: self size.
        count := 0.
        self do: [:each |
            (aBlock value: each) ifTrue: [
                count := count + 1.
                selected at: count put: each
            ]
        ].
        result := self species new: count.
        1 to: count do: [:index | result at: index put: (selected at: index)].
        ^result
    ]

    reject: aBlock [
        ^self select: [:each | (aBlock value: each) not]
    ]
]
