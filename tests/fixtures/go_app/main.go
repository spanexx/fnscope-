package main

import (
	"errors"
	"fmt"
)

// normalize trims surrounding whitespace from text.
func normalize(text string) string {
	if text == "" {
		panic("EMPTY_TEXT")
	}
	return text
}

// analyze validates text, normalizes it and logs the cleaned value.
func analyze(text string) (string, error) {
	// Guard against empty input before doing any work.
	if text == "" {
		return "", errors.New("EMPTY_TEXT")
	}
	cleaned := normalize(text)
	fmt.Println("cleaned:", cleaned)
	return cleaned, nil
}

// load normalizes text and propagates an error from analyze.
func load(text string) (string, error) {
	value, err := analyze(text)
	if err != nil {
		return "", err
	}
	return value, nil
}

// Service holds a prefix applied to text.
type Service struct {
	Prefix string
}

// Apply prefixes the supplied text.
func (s *Service) Apply(text string) string {
	return s.Prefix + normalize(text)
}

// report prints a cleaned value to standard output.
func report(text string) {
	fmt.Println(text)
}

func main() {
	service := &Service{Prefix: ">"}
	output := service.Apply("hello")
	report(output)
}
